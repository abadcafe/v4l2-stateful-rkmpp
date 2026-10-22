"""module: tests.support.syscalls_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_open_fd_missing_file_returns_rejected
    test_owned_fd_closes_once_and_retains_cloexec
    test_ioctl_existing_non_device_returns_enotty
    test_ioctl_accepts_valid_owned_descriptor


"""

import ctypes
import errno
import os
import termios
from pathlib import Path

import pytest
from rkmpp_bridge_uapi import EXCHANGE, VERSION, Batch

from tests.support.event_abi import Scalar
from tests.support.outcome import Ok, Rejected, accepted
from tests.support.syscalls import OwnedFd, command, descriptor_fits, ioctl, open_fd


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


def test_ioctl_oversized_descriptor_cannot_mutate_another_open_file() -> None:
    with OwnedFd(os.eventfd(0, os.EFD_CLOEXEC)) as owner:
        assert os.get_blocking(owner.fd)
        result = ioctl(owner.fd + (1 << 32), termios.FIONBIO, Scalar(value=1))
        assert isinstance(result, Rejected) and result.code == errno.EBADF
        assert os.get_blocking(owner.fd)


def test_ioctl_oversized_command_cannot_alias_another_operation() -> None:
    with OwnedFd(os.eventfd(0, os.EFD_CLOEXEC)) as owner:
        assert os.get_blocking(owner.fd)
        result = ioctl(owner.fd, termios.FIONBIO + (1 << 32), Scalar(value=1))
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
        assert os.get_blocking(owner.fd)


@pytest.mark.parametrize("fd", [-(1 << 128), 1 << 32, 1 << 128])
def test_ioctl_native_wire_rejects_unrepresentable_descriptor(fd: int) -> None:
    result = ioctl(fd, EXCHANGE, Batch(version=VERSION))
    assert isinstance(result, Rejected) and result.code == errno.EBADF


@pytest.mark.parametrize("request_code", [-1, 1 << 32, 1 << 128])
def test_ioctl_native_wire_rejects_unrepresentable_command(request_code: int) -> None:
    with accepted(open_fd(Path("/dev/null"))) as owner:
        result = ioctl(owner.fd, request_code, Batch(version=VERSION))
        assert isinstance(result, Rejected) and result.code == errno.EINVAL


@pytest.mark.parametrize("address", [-1, 1 << 128])
def test_ioctl_rejects_unrepresentable_argument_address(address: int) -> None:
    with accepted(open_fd(Path("/dev/null"))) as owner:
        result = ioctl(owner.fd, termios.FIONBIO, address)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL


@pytest.mark.parametrize(
    "address", [1, (1 << (ctypes.sizeof(ctypes.c_void_p) * 8)) - 1]
)
def test_ioctl_representable_inaccessible_address_reaches_kernel(address: int) -> None:
    with OwnedFd(os.eventfd(0, os.EFD_CLOEXEC)) as owner:
        result = ioctl(owner.fd, termios.FIONBIO, address)
        assert isinstance(result, Rejected) and result.code == errno.EFAULT
        assert os.get_blocking(owner.fd)


@pytest.mark.parametrize(
    ("fd", "expected"),
    [
        (0, True),
        ((1 << 31) - 1, True),
        (-1, False),
        (1 << 31, False),
        (1 << 128, False),
    ],
)
def test_descriptor_fits_native_signed_integer(fd: int, expected: bool) -> None:
    assert descriptor_fits(fd) is expected
