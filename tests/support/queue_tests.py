"""module: tests.support.queue_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_request_query_and_mapping_non_video_fd_return_enotty
    test_queue_dequeue_and_export_non_video_fd_return_enotty
    test_stream_on_and_stream_off_non_video_fd_return_enotty


"""

import ctypes
import errno
from pathlib import Path

import pytest

from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import open_fd


@pytest.mark.parametrize("value", [-1, 1 << 32, 1 << 128])
def test_queue_operations_reject_unrepresentable_unsigned_values(value: int) -> None:
    with accepted(open_fd(Path("/dev/null"))) as owner:
        queue = Queue(owner.fd, QueueType.OUTPUT)
        results = (
            queue.request(value),
            queue.query(value),
            queue.map(value),
            queue.queue(value),
            queue.queue(0, bytesused=value),
            queue.queue(0, data_offset=value),
            queue.export(value),
            queue.export(0, plane=value),
        )
        assert all(
            isinstance(result, Rejected) and result.code == errno.EINVAL
            for result in results
        )


@pytest.mark.parametrize("sign", [-1, 1])
def test_queue_rejects_timestamp_outside_native_timeval(sign: int) -> None:
    boundary = 1 << (8 * ctypes.sizeof(ctypes.c_long) - 1)
    timestamp = boundary * 1000000 if sign == 1 else -boundary * 1000000 - 1
    with accepted(open_fd(Path("/dev/null"))) as owner:
        result = Queue(owner.fd, QueueType.OUTPUT).queue(0, timestamp=timestamp)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_request_query_and_mapping_non_video_fd_return_enotty() -> None:
    with accepted(open_fd(Path("/dev/null"))) as owner:
        queue = Queue(owner.fd, QueueType.OUTPUT)
        for result in (queue.request(1), queue.query(0), queue.map(0)):
            assert isinstance(result, Rejected) and result.code == errno.ENOTTY


def test_queue_dequeue_and_export_non_video_fd_return_enotty() -> None:
    with accepted(open_fd(Path("/dev/null"))) as owner:
        queue = Queue(owner.fd, QueueType.OUTPUT)
        for result in (queue.queue(0), queue.dequeue(), queue.export(0)):
            assert isinstance(result, Rejected) and result.code == errno.ENOTTY


def test_stream_on_and_stream_off_non_video_fd_return_enotty() -> None:
    with accepted(open_fd(Path("/dev/null"))) as owner:
        queue = Queue(owner.fd, QueueType.OUTPUT)
        for result in (queue.stream_on(), queue.stream_off()):
            assert isinstance(result, Rejected) and result.code == errno.ENOTTY
