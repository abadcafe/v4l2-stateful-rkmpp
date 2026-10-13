"""module: kernel_peer_tests; responsibility: specify paired kernel session ownership.

operations: pytest cases; entities: none; tests: missing/exclusive/close contracts.
"""

import errno
from pathlib import Path

from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted
from tests.support.syscalls import ioctl, open_fd


def test_open_missing_broker_returns_rejected(tmp_path: Path) -> None:
    result = Peer.open(tmp_path / "missing", Path("/dev/video0"))
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_open_missing_video_releases_exclusive_broker(kernel_idle: Path) -> None:
    result = Peer.open(kernel_idle, Path("/missing-rkmpp-video"))
    assert isinstance(result, Rejected) and result.code == errno.ENOENT
    with accepted(open_fd(kernel_idle)):
        pass


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
