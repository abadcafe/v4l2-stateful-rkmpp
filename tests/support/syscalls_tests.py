"""module: tests.support.syscalls_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_open_fd_missing_file_returns_rejected
    test_owned_fd_closes_once_and_retains_cloexec
    test_ioctl_existing_non_device_returns_enotty
    test_ioctl_accepts_valid_owned_descriptor


"""

import errno
import os
from pathlib import Path

from tests.support.outcome import Ok, Rejected, accepted
from tests.support.syscalls import OwnedFd, command, ioctl, open_fd


def test_open_fd_missing_file_returns_rejected(tmp_path: Path) -> None:
    result = open_fd(tmp_path / "missing")
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_owned_fd_closes_once_and_retains_cloexec() -> None:
    fd = os.memfd_create("test-owned", os.MFD_CLOEXEC)
    with OwnedFd(fd) as owner:
        assert owner.fd == fd
        assert not os.get_inheritable(fd)
    owner.close()
    result = ioctl(fd, 0)
    assert isinstance(result, Rejected) and result.code == errno.EBADF


def test_ioctl_existing_non_device_returns_enotty() -> None:
    with accepted(open_fd(Path("/dev/null"))) as owner:
        result = ioctl(owner.fd, command(0, "R", 0x80))
        assert isinstance(result, Rejected) and result.code == errno.ENOTTY


def test_ioctl_accepts_valid_owned_descriptor() -> None:
    result = open_fd(Path("/dev/null"))
    assert isinstance(result, Ok)
    result.value.close()
