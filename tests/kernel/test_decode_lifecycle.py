"""module: test_decode_lifecycle; responsibility: specify real decoder continuation.

operations: pytest cases; entities: none; tests: same-stream seeks retain the
streaming CAPTURE queue and client-held image pixels for every codec.
"""

import errno
import hashlib
import time
from collections.abc import Generator
from contextlib import ExitStack
from pathlib import Path

import pytest

from tests.support.dmabuf import i420
from tests.support.outcome import Ok, accepted
from tests.support.queue import BufferInfo, Queue, QueueType
from tests.support.reference import frame_hashes
from tests.support.service_process import Lease
from tests.support.stream import Codec, load
from tests.support.video import Video
from tests.support.video_formats import get_format, set_format


@pytest.fixture
def decoding_video(
    kernel_idle: Path, executable: Path, tmp_path: Path
) -> Generator[Video]:
    with ExitStack() as owned:
        service = accepted(
            Lease.start(executable, kernel_idle, tmp_path / "status", tmp_path / "log")
        )
        _ = owned.callback(lambda: accepted(service.close()))
        video = owned.enter_context(accepted(Video.open(Path("/dev/video0"))))
        accepted(video.subscribe(5))
        yield video


def _receive(video: Video, capture: Queue) -> BufferInfo:
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        for kind, _ in accepted(video.events()):
            assert kind in (2, 5)
            if kind == 5:
                accepted(video.start())
        result = capture.dequeue()
        if isinstance(result, Ok):
            assert not result.value.flags & 0x40
            if result.value.bytesused != (0,):
                return result.value
            accepted(capture.queue(result.value.index))
        else:
            assert result.code in (errno.EAGAIN, errno.EPIPE)
        time.sleep(0.001)
    pytest.fail("CAPTURE did not produce the post-seek picture")


@pytest.mark.parametrize(
    "codec,file,depth",
    [
        (Codec.H264, "h264-yuv420p.h264", 8),
        (Codec.HEVC, "hevc-yuv420p.hevc", 8),
        (Codec.VP8, "vp8-yuv420p.ivf", 8),
        (Codec.VP9, "vp9-yuv420p.ivf", 8),
        (Codec.VP9, "vp9-yuv420p10le.ivf", 10),
        (Codec.AV1, "av1-yuv420p.ivf", 8),
        (Codec.AV1, "av1-yuv420p10le.ivf", 10),
    ],
)
def test_decode_output_seek_keeps_capture_streaming_and_held_pixels(
    decoding_video: Video,
    artifacts: Path,
    codec: Codec,
    file: str,
    depth: int,
) -> None:
    source = artifacts / file
    key = accepted(load(codec, source))[0].data
    expected = accepted(frame_hashes(source, depth, 256, 128))[0]
    with ExitStack() as owned:
        video = decoding_video
        _ = accepted(set_format(video.fd, QueueType.OUTPUT, codec.fourcc))
        output, capture = (
            Queue(video.fd, QueueType.OUTPUT),
            Queue(video.fd, QueueType.CAPTURE),
        )
        assert accepted(output.request(1)) == 1
        mapping = owned.enter_context(accepted(output.map(0)))
        mapping[: len(key)] = key
        accepted(output.stream_on())
        accepted(output.queue(0, len(key), 1000))
        deadline = time.monotonic() + 5
        while not accepted(video.events()):
            assert time.monotonic() < deadline
            time.sleep(0.001)
        layout = accepted(get_format(video.fd, QueueType.CAPTURE))
        count = accepted(capture.request(1))
        for index in range(count):
            accepted(capture.queue(index))
        accepted(capture.stream_on())
        accepted(video.stop())
        first = _receive(video, capture)
        assert first.timestamp == 1000
        held = owned.enter_context(accepted(capture.export(first.index)))
        pixels = accepted(i420(held.fd, 256, 128, layout.strides[0], layout.height))
        assert hashlib.sha256(pixels).hexdigest() == expected

        # Retain first's CAPTURE storage in the client throughout the seek.
        accepted(output.stream_off())
        mapping[: len(key)] = key
        accepted(output.stream_on())
        accepted(output.queue(0, len(key), 2000))
        accepted(video.stop())
        second = _receive(video, capture)
        assert second.timestamp == 2000 and second.index != first.index
        current = owned.enter_context(accepted(capture.export(second.index)))
        pixels = accepted(i420(current.fd, 256, 128, layout.strides[0], layout.height))
        assert hashlib.sha256(pixels).hexdigest() == expected
        pixels = accepted(i420(held.fd, 256, 128, layout.strides[0], layout.height))
        assert hashlib.sha256(pixels).hexdigest() == expected


@pytest.mark.parametrize(
    "codec,file,depth",
    [
        (Codec.VP8, "vp8-yuv420p.ivf", 8),
        (Codec.VP9, "vp9-yuv420p.ivf", 8),
        (Codec.VP9, "vp9-yuv420p10le.ivf", 10),
        (Codec.AV1, "av1-yuv420p.ivf", 8),
        (Codec.AV1, "av1-yuv420p10le.ivf", 10),
    ],
)
def test_decode_last_requeue_before_resume_preserves_reference_pixels(
    decoding_video: Video, artifacts: Path, codec: Codec, file: str, depth: int
) -> None:
    source = artifacts / file
    packets = accepted(load(codec, source))[:2]
    expected = accepted(frame_hashes(source, depth, 256, 128))[:2]
    video = decoding_video
    _ = accepted(set_format(video.fd, QueueType.OUTPUT, codec.fourcc))
    output, capture = (
        Queue(video.fd, QueueType.OUTPUT),
        Queue(video.fd, QueueType.CAPTURE),
    )
    assert accepted(output.request(1)) == 1
    with accepted(output.map(0)) as mapping:
        mapping[: len(packets[0].data)] = packets[0].data
        accepted(output.stream_on())
        accepted(output.queue(0, len(packets[0].data), 1000))
        deadline = time.monotonic() + 5
        while not accepted(video.events()):
            assert time.monotonic() < deadline
            time.sleep(0.001)
        layout = accepted(get_format(video.fd, QueueType.CAPTURE))
        for index in range(accepted(capture.request(1))):
            accepted(capture.queue(index))
        accepted(capture.stream_on())
        accepted(video.stop())
        first = _receive(video, capture)
        with accepted(capture.export(first.index)) as descriptor:
            pixels = accepted(
                i420(descriptor.fd, 256, 128, layout.strides[0], layout.height)
            )
        assert hashlib.sha256(pixels).hexdigest() == expected[0]
        accepted(capture.queue(first.index))
        # Drain completion includes returning the preceding OUTPUT buffer.
        while True:
            completed = output.dequeue()
            if isinstance(completed, Ok):
                break
            assert completed.code == errno.EAGAIN and time.monotonic() < deadline
            time.sleep(0.001)
        while True:
            completed = capture.dequeue()
            if isinstance(completed, Ok):
                break
            assert completed.code == errno.EAGAIN and time.monotonic() < deadline
            time.sleep(0.001)
        assert completed.value.bytesused == (0,) and completed.value.flags & 0x100000
        # This legal QBUF must not be mistaken for a duplicate of the permission
        # the kernel consumed when it produced its empty LAST.
        accepted(capture.queue(completed.value.index))
        accepted(video.start())
        mapping[: len(packets[1].data)] = packets[1].data
        accepted(output.queue(0, len(packets[1].data), 2000))
        accepted(video.stop())
        second = _receive(video, capture)
        assert second.timestamp == 2000
        with accepted(capture.export(second.index)) as descriptor:
            pixels = accepted(
                i420(descriptor.fd, 256, 128, layout.strides[0], layout.height)
            )
        assert hashlib.sha256(pixels).hexdigest() == expected[1]
