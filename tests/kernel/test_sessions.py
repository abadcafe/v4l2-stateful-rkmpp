"""module: test_sessions; responsibility: specify broker session isolation and fd failures.

operations: pytest cases; entities: none;
tests: FIFO accept, cancelled pending sessions, per-session poisoning and EMFILE rollback.
"""

import errno
import os
import resource
from contextlib import ExitStack
from pathlib import Path

from tests.support.bridge import Session
from tests.support.bridge_abi import ACCEPT, VERSION, Event, Message, Reply
from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import OwnedFd, ioctl, open_fd
from tests.support.video import Video


def test_accept_pending_video_sessions_preserves_fifo_and_independent_poisoning(
    kernel_owner: Path,
) -> None:
    with accepted(open_fd(kernel_owner)) as broker, ExitStack() as owned:
        videos = [
            owned.enter_context(accepted(Video.open(Path("/dev/video0"))))
            for _ in range(3)
        ]
        for video in videos:
            accepted(video.start())
        peers = [Session(OwnedFd(accepted(ioctl(broker.fd, ACCEPT)))) for _ in videos]
        for peer in peers:
            _ = owned.callback(peer.close)
            event = Message.from_buffer_copy(accepted(peer.exchange()).events)
            assert event.kind == Event.RESUME
        peers[1].close()
        result = Queue(videos[1].fd, QueueType.OUTPUT).request(1)
        assert isinstance(result, Rejected) and result.code == errno.EIO
        for index in (0, 2):
            assert accepted(Queue(videos[index].fd, QueueType.OUTPUT).request(1)) == 1
            videos[index].close()
            assert (
                Message.from_buffer_copy(accepted(peers[index].exchange()).events).kind
                == Event.CLOSE
            )


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
        result = Queue(video.fd, QueueType.OUTPUT).request(1)
        assert isinstance(result, Rejected) and result.code == errno.EIO
        empty = ioctl(broker.fd, ACCEPT)
        assert isinstance(empty, Rejected) and empty.code == errno.EAGAIN


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
