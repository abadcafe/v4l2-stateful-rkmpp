"""module: syscalls; responsibility: own Linux syscall resources in acceptance tests.

operations: OwnedFd, WireBuffer, open_fd, ioctl, command;
entities: OwnedFd; tests: syscalls_tests and kernel/test_copyout_faults.
"""

import ctypes
import os
from pathlib import Path
from types import TracebackType
from typing import Protocol

from pydantic import TypeAdapter

from tests.support.outcome import Ok, Rejected, Result

_libc = ctypes.CDLL(None, use_errno=True)
_libc.ioctl.restype = ctypes.c_int


class WireBuffer(Protocol):
    def ioctl(self, fd: int, request: int) -> int: ...


class OwnedFd:
    def __init__(self, fd: int) -> None:
        assert fd >= 0
        self._fd: int | None = fd

    @property
    def fd(self) -> int:
        assert self._fd is not None
        return self._fd

    def close(self) -> None:
        if self._fd is not None:
            os.close(self._fd)
            self._fd = None

    def __enter__(self) -> OwnedFd:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()


def open_fd(path: Path) -> Result[OwnedFd]:
    try:
        fd = os.open(path, os.O_RDWR | os.O_NONBLOCK | os.O_CLOEXEC)
    except OSError as error:
        return Rejected(error.errno or 0, f"open {path}")
    return Ok(OwnedFd(fd))


def command(direction: int, letter: str, number: int, size: int = 0) -> int:
    assert 0 <= direction <= 3 and len(letter) == 1
    assert 0 <= number <= 255 and 0 <= size < 16384
    return direction << 30 | size << 16 | ord(letter) << 8 | number


def ioctl(
    fd: int, request: int, payload: ctypes.Structure | WireBuffer | int = 0
) -> Result[int]:
    if not isinstance(payload, (ctypes.Structure, int)):
        result = payload.ioctl(fd, request)
        return Rejected(-result, f"ioctl {request:#x}") if result < 0 else Ok(result)
    address = (
        ctypes.addressof(payload) if isinstance(payload, ctypes.Structure) else payload
    )
    # A direct pointer is essential: fcntl's temporary copy masks copyout EFAULT.
    result = TypeAdapter(int).validate_python(
        _libc.ioctl(
            ctypes.c_int(fd), ctypes.c_ulong(request), ctypes.c_void_p(address)
        ),
        strict=True,
    )
    if result < 0:
        return Rejected(ctypes.get_errno(), f"ioctl {request:#x}")
    return Ok(result)
