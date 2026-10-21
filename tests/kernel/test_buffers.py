"""module: test_buffers; responsibility: specify kernel buffer allocation and ownership.

operations: pytest cases; entities: none;
tests: capacities, memory modes, invalid descriptors, shared mmap/export and queue ownership.
"""

import ctypes
import errno

import pytest

from tests.support.buffer_abi import Plane, VideoBuffer
from tests.support.dmabuf import read_bytes
from tests.support.format_abi import RequestBuffers
from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import command, ioctl
from tests.support.video import Video


@pytest.mark.parametrize(
    ("kind", "requested", "expected"),
    [
        (QueueType.OUTPUT, 1, 1),
        (QueueType.OUTPUT, 33, 32),
        (QueueType.CAPTURE, 1, 2),
        (QueueType.CAPTURE, 33, 32),
    ],
)
def test_request_queue_capacity_and_capture_minimum(
    video: Video,
    kind: QueueType,
    requested: int,
    expected: int,
) -> None:
    queue = Queue(video.fd, kind)
    assert accepted(queue.request(requested)) == expected
    assert accepted(queue.query(expected - 1)).index == expected - 1
    result = queue.query(expected)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    assert accepted(queue.request(0)) == 0


@pytest.mark.parametrize(
    ("kind", "memory"), [(1, 1), (2, 1), (9, 2), (9, 4), (10, 2), (10, 4)]
)
def test_request_unsupported_type_and_memory_reject_without_allocating(
    video: Video,
    kind: int,
    memory: int,
) -> None:
    payload = RequestBuffers(count=2, kind=kind, memory=memory)
    result = ioctl(video.fd, command(3, "V", 8, ctypes.sizeof(payload)), payload)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    assert accepted(Queue(video.fd, QueueType.OUTPUT).request(1)) == 1


@pytest.mark.parametrize(
    ("length", "address", "expected"),
    [
        (0, 0, errno.EINVAL),
        (1, 0, errno.EFAULT),
        (1, 1, errno.EFAULT),
    ],
)
def test_query_invalid_plane_array_does_not_change_allocated_queue(
    video: Video,
    length: int,
    address: int,
    expected: int,
) -> None:
    queue = Queue(video.fd, QueueType.OUTPUT)
    assert accepted(queue.request(2)) == 2
    payload = VideoBuffer(kind=10, memory=1, length=length)
    payload.location.planes = address
    result = ioctl(video.fd, command(3, "V", 9, ctypes.sizeof(payload)), payload)
    assert isinstance(result, Rejected) and result.code == expected
    assert accepted(queue.query(0)).lengths[0] == 1024 * 1024


def test_queue_payload_overflow_and_duplicate_slot_are_rejected(video: Video) -> None:
    queue = Queue(video.fd, QueueType.OUTPUT)
    assert accepted(queue.request(2)) == 2
    result = queue.queue(0, 1024 * 1024 + 1)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    accepted(queue.queue(0, 16, 123456))
    result = queue.queue(0, 16)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    assert accepted(queue.query(0)).flags & 2


def test_map_and_export_share_storage_and_queue_offsets_never_alias(
    video: Video,
) -> None:
    output, capture = (
        Queue(video.fd, QueueType.OUTPUT),
        Queue(video.fd, QueueType.CAPTURE),
    )
    assert accepted(output.request(2)) == 2
    assert accepted(capture.request(1)) == 2
    first, second = accepted(output.query(0)), accepted(capture.query(0))
    assert first.offsets[0] != second.offsets[0]
    with accepted(output.map(0)) as a, accepted(capture.map(0)) as b:
        a[:16], b[:16] = b"A" * 16, b"B" * 16
        with accepted(output.export(0)) as a_fd, accepted(capture.export(0)) as b_fd:
            assert accepted(read_bytes(a_fd.fd, 16)) == b"A" * 16
            assert accepted(read_bytes(b_fd.fd, 16)) == b"B" * 16
            assert accepted(output.request(0)) == 0
            assert a[:16] == b"A" * 16
            assert accepted(read_bytes(a_fd.fd, 16)) == b"A" * 16
            assert accepted(output.request(1)) == 1
            with accepted(output.map(0)) as replacement:
                replacement[:16] = b"C" * 16
                assert a[:16] == b"A" * 16
                assert accepted(read_bytes(a_fd.fd, 16)) == b"A" * 16
    assert accepted(output.request(0)) == accepted(capture.request(0)) == 0


def test_query_plane_bytes_and_timeval_roundtrip_queued_output(video: Video) -> None:
    queue = Queue(video.fd, QueueType.OUTPUT)
    assert accepted(queue.request(1)) == 1
    accepted(queue.queue(0, 256, 9123456))
    planes = (Plane * 1)()
    payload = VideoBuffer(kind=10, memory=1, length=1)
    payload.location.planes = ctypes.addressof(planes)
    assert (
        accepted(ioctl(video.fd, command(3, "V", 9, ctypes.sizeof(payload)), payload))
        == 0
    )
    assert Plane.from_buffer(planes).bytesused == 256
    assert (payload.timestamp.seconds, payload.timestamp.microseconds) == (9, 123456)


def test_all_buffer_cookies_map_distinct_storage(video: Video) -> None:
    queues = (Queue(video.fd, QueueType.OUTPUT), Queue(video.fd, QueueType.CAPTURE))
    cookies: set[int] = set()
    for number, queue in enumerate(queues):
        assert accepted(queue.request(32)) == 32
        for index in range(32):
            info = accepted(queue.query(index))
            assert info.offsets[0] not in cookies
            cookies.add(info.offsets[0])
            with accepted(queue.map(index)) as memory:
                memory[:1] = bytes([number * 32 + index])
    for number, queue in enumerate(queues):
        for index in range(32):
            with accepted(queue.map(index)) as memory:
                assert memory[:1] == bytes([number * 32 + index])
            with accepted(queue.export(index)) as exported:
                assert accepted(read_bytes(exported.fd, 1)) == bytes(
                    [number * 32 + index]
                )
