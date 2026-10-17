"""module: tests.kernel.test_formats; responsibility: specify V4L2 format boundaries.

operations: pytest cases; entities: none; tests:
    test_video_capability_and_format_enumeration
    test_set_format_clamps_extent_and_packet_size
    test_set_format_allocated_queue_is_busy_and_release_allows_change
    test_queue_invalid_slot_export_and_empty_dequeue_return_specific_errors
    test_video_unsupported_event_subscription_is_rejected

Actual format, queue and error boundaries of the bridge video node.
"""

import ctypes
import errno
import os
import struct
import time
from concurrent.futures import ThreadPoolExecutor

import pytest

from tests.support.bridge_abi import Message, Reply
from tests.support.format_abi import Capability, Format, FormatDescription, Selection
from tests.support.framesize_abi import FrameSize
from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType
from tests.support.stream import Codec
from tests.support.syscalls import command, ioctl
from tests.support.video import Video
from tests.support.video_formats import (
    enumerate_formats,
    get_format,
    selection,
    set_format,
)


def test_video_capability_and_format_enumeration(video: Video) -> None:
    capability: Capability = accepted(video.capability())
    assert bytes(capability.driver).split(b"\0", 1)[0] == b"v4l2_rkmpp"
    assert bytes(capability.card).split(b"\0", 1)[0] == b"v4l2_rkmpp_bridge"
    assert (
        bytes(capability.bus_info).split(b"\0", 1)[0] == b"platform:v4l2_rkmpp_bridge"
    )
    assert capability.device_caps & 0x4004000 == 0x4004000
    assert set(accepted(enumerate_formats(video.fd, QueueType.OUTPUT))) == {
        codec.fourcc for codec in Codec
    }
    assert accepted(enumerate_formats(video.fd, QueueType.CAPTURE)) == [
        int.from_bytes(b"NV12", "little")
    ]
    description = FormatDescription(index=4, kind=QueueType.OUTPUT)
    _ = accepted(
        ioctl(video.fd, command(3, "V", 2, ctypes.sizeof(description)), description)
    )
    assert bytes(description)[12:44].split(b"\0", 1)[0] == b"AV1 OBU Stream"
    capture = accepted(get_format(video.fd, QueueType.CAPTURE))
    assert (
        capture.width,
        capture.height,
        capture.planes,
        capture.strides,
        capture.sizes,
    ) == (64, 64, 1, (64,), (6144,))
    assert accepted(selection(video.fd, QueueType.CAPTURE)) == (64, 64)


@pytest.mark.parametrize("index", range(5))
def test_enumerate_formats_declares_input_framing(video: Video, index: int) -> None:
    description = FormatDescription(index=index, kind=QueueType.OUTPUT)
    _ = accepted(
        ioctl(video.fd, command(3, "V", 2, ctypes.sizeof(description)), description)
    )
    assert description.flags & 4 == 0


@pytest.mark.parametrize(
    ("width", "height", "size", "expected"),
    [
        (0, 0, 0, (64, 64, 1024 * 1024)),
        (64, 8192, 16 * 1024 * 1024, (64, 8192, 16 * 1024 * 1024)),
        (9999, 9999, 32 * 1024 * 1024, (8192, 8192, 16 * 1024 * 1024)),
    ],
)
def test_set_format_clamps_extent_and_packet_size(
    video: Video, width: int, height: int, size: int, expected: tuple[int, int, int]
) -> None:
    chosen = accepted(
        set_format(
            video.fd,
            QueueType.OUTPUT,
            Codec.H264.fourcc,
            width=width,
            height=height,
            size=size,
        )
    )
    assert (chosen.width, chosen.height, chosen.sizes[0]) == expected


def test_set_format_allocated_queue_is_busy_and_release_allows_change(
    video: Video,
) -> None:
    queue = Queue(video.fd, QueueType.OUTPUT)
    assert accepted(queue.request(3)) == 3
    rejected = set_format(video.fd, QueueType.OUTPUT, Codec.VP9.fourcc)
    assert isinstance(rejected, Rejected) and rejected.code == errno.EBUSY
    assert accepted(queue.request(0)) == 0
    assert (
        accepted(set_format(video.fd, QueueType.OUTPUT, Codec.VP9.fourcc)).fourcc
        == Codec.VP9.fourcc
    )


def test_queue_invalid_slot_export_and_empty_dequeue_return_specific_errors(
    raw_session: Peer,
) -> None:
    video = raw_session.video
    queue = Queue(video.fd, QueueType.OUTPUT)
    assert accepted(queue.request(3)) == 3
    for result in (queue.query(3), queue.queue(3), queue.export(0, 1)):
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
    result = queue.dequeue()
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    accepted(queue.stream_on())
    result = queue.dequeue()
    assert isinstance(result, Rejected) and result.code == errno.EAGAIN
    with ThreadPoolExecutor(max_workers=1) as executor:
        stopped = executor.submit(queue.stream_off)
        deadline = time.monotonic() + 5
        while True:
            batch = accepted(raw_session.service.exchange())
            if batch.event_count:
                break
            assert time.monotonic() < deadline
            time.sleep(0.001)
        acknowledgement = Message.from_buffer_copy(batch.events)
        acknowledgement.kind = Reply.STOPPED
        assert accepted(raw_session.service.exchange([acknowledgement])).results[0] == 0
        accepted(stopped.result(timeout=5))
    with accepted(queue.export(0)) as owner:
        assert (
            not os.get_inheritable(owner.fd)
            and os.fstat(owner.fd).st_size >= 1024 * 1024
        )
    assert accepted(queue.request(0)) == 0


def test_video_unsupported_event_subscription_is_rejected(video: Video) -> None:
    result = video.subscribe(999)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    assert accepted(video.stop()) is None
    assert accepted(video.events()) == []
    assert ctypes.sizeof(accepted(video.capability())) == 104


@pytest.mark.parametrize("codec", list(Codec))
def test_format_frame_size_enumeration_publishes_full_extent_range(
    video: Video,
    codec: Codec,
) -> None:
    size = FrameSize(fourcc=codec.fourcc)
    assert (
        accepted(ioctl(video.fd, command(3, "V", 74, ctypes.sizeof(size)), size)) == 0
    )
    limits = size.stepwise
    assert (
        size.kind,
        limits.min_width,
        limits.max_width,
        limits.step_width,
        limits.min_height,
        limits.max_height,
        limits.step_height,
    ) == (3, 64, 8192, 1, 64, 8192, 1)
    size.index = 1
    result = ioctl(video.fd, command(3, "V", 74, ctypes.sizeof(size)), size)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_format_try_does_not_mutate_current_format_and_unknown_codec_falls_back(
    video: Video,
) -> None:
    original = accepted(get_format(video.fd, QueueType.OUTPUT))
    wire = Format(kind=QueueType.OUTPUT)
    data = bytearray(200)
    struct.pack_into("<III", data, 0, 9999, 0, int.from_bytes(b"NOPE", "little"))
    wire.fmt.data[:] = data
    assert (
        accepted(ioctl(video.fd, command(3, "V", 64, ctypes.sizeof(wire)), wire)) == 0
    )
    assert struct.unpack_from("<III", bytes(wire.fmt.data)) == (
        8192,
        64,
        Codec.H264.fourcc,
    )
    assert accepted(get_format(video.fd, QueueType.OUTPUT)) == original
    assert (
        accepted(set_format(video.fd, QueueType.OUTPUT, 0)).fourcc == Codec.H264.fourcc
    )


@pytest.mark.parametrize(("kind", "target"), [(10, 0x100), (9, 0xFFFF)])
def test_selection_invalid_queue_or_target_is_rejected(
    video: Video, kind: int, target: int
) -> None:
    payload = Selection(kind=kind, target=target)
    result = ioctl(video.fd, command(3, "V", 94, ctypes.sizeof(payload)), payload)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


@pytest.mark.parametrize(("width", "height"), [(1920, 1080), (1280, 720)])
def test_output_format_immediately_updates_provisional_capture(
    video: Video, width: int, height: int
) -> None:
    _ = accepted(
        set_format(
            video.fd, QueueType.OUTPUT, Codec.H264.fourcc, width=width, height=height
        )
    )
    capture = accepted(get_format(video.fd, QueueType.CAPTURE))
    assert capture.width >= width and capture.height >= height
    assert capture.strides[0] >= width
    assert capture.sizes[0] >= capture.strides[0] * capture.height * 3 // 2
    assert accepted(selection(video.fd, QueueType.CAPTURE)) == (width, height)
    for target in (0, 1, 2):
        value = Selection(kind=QueueType.CAPTURE, target=target)
        _ = accepted(ioctl(video.fd, command(3, "V", 94, ctypes.sizeof(value)), value))


def test_output_format_cannot_invalidate_allocated_capture(video: Video) -> None:
    capture = Queue(video.fd, QueueType.CAPTURE)
    _ = accepted(capture.request(18))
    result = set_format(video.fd, QueueType.OUTPUT, Codec.HEVC.fourcc)
    assert isinstance(result, Rejected) and result.code == errno.EBUSY
