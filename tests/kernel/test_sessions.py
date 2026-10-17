"""module: test_sessions; responsibility: specify broker session isolation and fd failures.

operations: pytest cases; entities: none;
tests: FIFO accept, cancelled pending sessions, per-session poisoning and EMFILE rollback.
"""

import errno
import os
import resource
import select
from contextlib import ExitStack
from pathlib import Path

import pytest

from tests.support.bridge import Session
from tests.support.bridge_abi import (
    ACCEPT,
    EXCHANGE,
    VERSION,
    Batch,
    Event,
    Message,
    Reply,
)
from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import OwnedFd, ioctl, open_fd
from tests.support.video import Video
from tests.support.video_formats import get_format, set_format


@pytest.mark.parametrize("kind", list(Reply))
@pytest.mark.parametrize("same_batch", [False, True])
def test_failed_session_rejects_every_reply_without_state_mutation(
    raw_session: Peer, kind: Reply, same_batch: bool
) -> None:
    peer = raw_session
    accepted(peer.video.subscribe(5))
    before = accepted(get_format(peer.video.fd, QueueType.CAPTURE))
    failed = Message(version=VERSION, kind=Reply.FAILED, epoch=1)
    following = Message(
        version=VERSION,
        kind=kind,
        epoch=1,
        width=128,
        height=64,
        coded_width=128,
        coded_height=64,
        stride=128,
        rows=64,
        size=12288,
        format=1,
    )
    if not same_batch:
        result = peer.service.exchange([failed])
        assert isinstance(result, Rejected) and result.code == errno.EPIPE
    batch = Batch(version=VERSION, reply_count=2 if same_batch else 1)
    if same_batch:
        batch.replies[0] = failed
    batch.replies[batch.reply_count - 1] = following
    result = ioctl(peer.service.fd, EXCHANGE, batch)
    assert isinstance(result, Rejected) and result.code == errno.EPIPE
    assert list(batch.results[: batch.reply_count]) == (
        [0, -errno.EPIPE] if same_batch else [-errno.EPIPE]
    )
    assert accepted(get_format(peer.video.fd, QueueType.CAPTURE)) == before
    assert accepted(peer.video.events()) == []
    for result in (
        peer.video.start(),
        peer.video.stop(),
        set_format(peer.video.fd, QueueType.OUTPUT, int.from_bytes(b"H264", "little")),
        Queue(peer.video.fd, QueueType.OUTPUT).stream_on(),
    ):
        assert isinstance(result, Rejected) and result.code == errno.EIO


def test_accept_pending_video_sessions_preserves_fifo_and_independent_poisoning(
    kernel_owner: Path,
) -> None:
    with accepted(open_fd(kernel_owner)) as broker, ExitStack() as owned:
        videos = [
            owned.enter_context(accepted(Video.open(Path("/dev/video0"))))
            for _ in range(3)
        ]
        for index, video in enumerate(videos):
            queue = Queue(video.fd, QueueType.OUTPUT)
            assert accepted(queue.request(1)) == 1
            accepted(queue.stream_on())
            accepted(queue.queue(0, 16, index * 1000))
        peers = [Session(OwnedFd(accepted(ioctl(broker.fd, ACCEPT)))) for _ in videos]
        events: list[Message] = []
        for index, peer in enumerate(peers):
            _ = owned.callback(peer.close)
            event = Message.from_buffer_copy(accepted(peer.exchange()).events)
            assert event.kind == Event.PACKET and event.timestamp == index * 1000000
            events.append(event)
        peers[1].close()
        result = Queue(videos[1].fd, QueueType.OUTPUT).request(1)
        assert isinstance(result, Rejected) and result.code == errno.EIO
        for index in (0, 2):
            reply = Message.from_buffer_copy(events[index])
            reply.kind = Reply.INPUT_DONE
            assert accepted(peers[index].exchange([reply])).results[0] == 0
            queue = Queue(videos[index].fd, QueueType.OUTPUT)
            assert accepted(queue.dequeue()).index == 0
            accepted(queue.queue(0, 16))
            assert accepted(peers[index].exchange()).event_count == 1


def test_accept_closed_pending_video_is_removed_and_next_video_remains_usable(
    kernel_owner: Path,
) -> None:
    with accepted(open_fd(kernel_owner)) as broker:
        first = accepted(Video.open(Path("/dev/video0")))
        with accepted(Video.open(Path("/dev/video0"))) as second:
            first.close()
            peer = Session(OwnedFd(accepted(ioctl(broker.fd, ACCEPT))))
            try:
                assert accepted(peer.exchange()).event_count == 0
                empty = ioctl(broker.fd, ACCEPT)
                assert isinstance(empty, Rejected) and empty.code == errno.EAGAIN
                assert accepted(Queue(second.fd, QueueType.OUTPUT).request(1)) == 1
            finally:
                peer.close()


def test_accept_fd_exhaustion_poisoning_releases_failed_session_reference(
    kernel_owner: Path,
) -> None:
    with (
        accepted(open_fd(kernel_owner)) as broker,
        accepted(Video.open(Path("/dev/video0"))) as video,
    ):
        output = Queue(video.fd, QueueType.OUTPUT)
        assert accepted(output.request(1)) == 1
        accepted(output.stream_on())
        accepted(output.queue(0, 16))
        old_limit = resource.getrlimit(resource.RLIMIT_NOFILE)
        temporary_limit = max(
            64, max(int(entry.name) for entry in Path("/proc/self/fd").iterdir()) + 16
        )
        assert temporary_limit < old_limit[0]
        fds: list[OwnedFd] = []
        try:
            resource.setrlimit(resource.RLIMIT_NOFILE, (temporary_limit, old_limit[1]))
            while True:
                opened = open_fd(Path("/dev/null"))
                if isinstance(opened, Rejected):
                    assert opened.code == errno.EMFILE
                    break
                fds.append(opened.value)
            result = ioctl(broker.fd, ACCEPT)
            assert isinstance(result, Rejected) and result.code == errno.EMFILE
        finally:
            for fd in fds:
                fd.close()
            resource.setrlimit(resource.RLIMIT_NOFILE, old_limit)
        assert video.poll(0) & select.POLLERR
        for result in (output.dequeue(), video.start(), output.queue(0, 16)):
            assert isinstance(result, Rejected) and result.code == errno.EIO
        empty = ioctl(broker.fd, ACCEPT)
        assert isinstance(empty, Rejected) and empty.code == errno.EAGAIN
        # A failed admission must not strand another client or poison the broker.
        with accepted(Video.open(Path("/dev/video0"))) as other:
            peer = Session(OwnedFd(accepted(ioctl(broker.fd, ACCEPT))))
            try:
                assert accepted(peer.exchange()).event_count == 0
                assert accepted(Queue(other.fd, QueueType.OUTPUT).request(1)) == 1
            finally:
                peer.close()


def test_failed_reply_has_no_effect_on_another_open_session(kernel_owner: Path) -> None:
    with accepted(open_fd(kernel_owner)) as broker, ExitStack() as owned:
        videos = [
            owned.enter_context(accepted(Video.open(Path("/dev/video0"))))
            for _ in range(2)
        ]
        peers = [Session(OwnedFd(accepted(ioctl(broker.fd, ACCEPT)))) for _ in videos]
        for peer in peers:
            _ = owned.callback(peer.close)
        failed = Message(version=VERSION, kind=Reply.FAILED, epoch=1)
        ended = peers[0].exchange([failed])
        assert isinstance(ended, Rejected) and ended.code == errno.EPIPE
        assert accepted(Queue(videos[1].fd, QueueType.OUTPUT).request(1)) == 1
        result = Queue(videos[0].fd, QueueType.OUTPUT).request(1)
        assert isinstance(result, Rejected) and result.code == errno.EIO
        assert not os.get_inheritable(peers[1].fd)
