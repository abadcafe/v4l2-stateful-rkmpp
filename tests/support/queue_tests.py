"""module: tests.support.queue_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_request_query_and_mapping_non_video_fd_return_enotty
    test_queue_dequeue_and_export_non_video_fd_return_enotty
    test_stream_on_and_stream_off_non_video_fd_return_enotty


"""

import errno
from pathlib import Path

from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import open_fd


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
