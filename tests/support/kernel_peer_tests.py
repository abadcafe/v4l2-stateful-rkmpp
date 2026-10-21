"""module: kernel_peer_tests; responsibility: specify paired kernel session ownership.

operations: pytest cases; entities: none; tests: missing/exclusive/close contracts.
"""

import errno
from contextlib import ExitStack
from pathlib import Path

import pytest
from rkmpp_bridge_uapi import ACCEPT

from tests.support.bridge import Session
from tests.support.kernel_peer import Peer
from tests.support.outcome import Ok, Rejected, Result, accepted
from tests.support.syscalls import OwnedFd, ioctl, open_fd
from tests.support.video import Video


def test_open_missing_broker_returns_rejected(tmp_path: Path) -> None:
    result = Peer.open(tmp_path / "missing", Path("/dev/video0"))
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_open_missing_video_releases_exclusive_broker(kernel_idle: Path) -> None:
    result = Peer.open(kernel_idle, Path("/missing-rkmpp-video"))
    assert isinstance(result, Rejected) and result.code == errno.ENOENT
    with accepted(open_fd(kernel_idle)):
        pass


def test_open_rejects_pending_client_without_accepting_its_session(
    kernel_idle: Path,
) -> None:
    with accepted(Video.open(Path("/dev/video0"))) as waiting:
        assert accepted(waiting.capability()).device_caps & 0x4000
        result = Peer.open(kernel_idle, Path("/dev/video0"))
        if isinstance(result, Ok):
            result.value.close()
        assert isinstance(result, Rejected) and result.code == errno.EBUSY
        with accepted(open_fd(kernel_idle)) as broker:
            endpoint = accepted(ioctl(broker.fd, ACCEPT))
            with Session(OwnedFd(endpoint)) as session:
                assert accepted(session.exchange()).event_count == 0


def test_close_pair_releases_both_descriptors_and_exclusive_broker(
    kernel_idle: Path,
) -> None:
    with accepted(Peer.open(kernel_idle, Path("/dev/video0"))) as peer:
        video_fd, service_fd = peer.video.fd, peer.service.fd
        assert accepted(peer.video.capability()).device_caps & 0x4000
        assert accepted(peer.service.exchange()).event_count == 0
        duplicate = open_fd(kernel_idle)
        assert isinstance(duplicate, Rejected) and duplicate.code == errno.EBUSY
    peer.close()
    for fd in (video_fd, service_fd):
        result = ioctl(fd, 0)
        assert isinstance(result, Rejected) and result.code == errno.EBADF
    with accepted(open_fd(kernel_idle)):
        pass


def test_open_rejects_client_arriving_during_pair_acquisition(
    kernel_idle: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    open_video = Video.open
    competing: list[Video] = []
    with ExitStack() as owned:

        def concurrent_open(path: Path) -> Result[Video]:
            client = owned.enter_context(accepted(open_video(path)))
            competing.append(client)
            return open_video(path)

        monkeypatch.setattr(Video, "open", staticmethod(concurrent_open))
        result = Peer.open(kernel_idle, Path("/dev/video0"))
        if isinstance(result, Ok):
            result.value.close()
        assert len(competing) == 1
        assert isinstance(result, Rejected) and result.code == errno.EBUSY
        with accepted(open_fd(kernel_idle)):
            pass
