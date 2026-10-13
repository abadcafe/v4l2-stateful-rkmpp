"""module: syscalls; responsibility: own Linux syscall resources in acceptance tests.

operations: OwnedFd, open_fd, ioctl, command, readonly_payload;
entities: OwnedFd; tests: syscalls_tests and kernel/test_copyout_faults.
"""

import ctypes
import mmap
import os
from collections.abc import Generator
from contextlib import contextmanager
from pathlib import Path
from types import TracebackType

from pydantic import TypeAdapter

from tests.support.outcome import Ok, Rejected, Result

_libc = ctypes.CDLL(None, use_errno=True)
_libc.ioctl.restype = ctypes.c_int
_libc.mprotect.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int]
_libc.mprotect.restype = ctypes.c_int


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


def ioctl(fd: int, request: int, payload: ctypes.Structure | int = 0) -> Result[int]:
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


@contextmanager
def readonly_payload(payload: ctypes.Structure) -> Generator[int]:
    size = ctypes.sizeof(payload)
    with mmap.mmap(-1, size) as mapping:
        mapping[:] = ctypes.string_at(ctypes.addressof(payload), size)
        anchor = ctypes.c_char.from_buffer(mapping)
        address = ctypes.addressof(anchor)
        del anchor
        assert (
            TypeAdapter(int).validate_python(
                _libc.mprotect(ctypes.c_void_p(address), size, mmap.PROT_READ),
                strict=True,
            )
            == 0
        )
        try:
            yield address
        finally:
            assert (
                TypeAdapter(int).validate_python(
                    _libc.mprotect(
                        ctypes.c_void_p(address), size, mmap.PROT_READ | mmap.PROT_WRITE
                    ),
                    strict=True,
                )
                == 0
            )
