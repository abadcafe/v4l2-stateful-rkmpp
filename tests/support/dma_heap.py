"""module: dma_heap; responsibility: own synchronized mock DMA allocations.

operations: Buffer allocate/fd/size/write/close; entities: Buffer;
tests: dma_heap_tests, mocked kernel process decoding.
"""

import ctypes
import errno
import mmap
import os
from pathlib import Path
from types import TracebackType
from typing import final

from tests.support.dma_sync import finish_write, start_write
from tests.support.outcome import Ok, Rejected, Result
from tests.support.syscalls import OwnedFd, command, ioctl, open_fd


@final
class _Allocation(ctypes.Structure):
    length: int = 0
    fd: int = 0
    fd_flags: int = 0
    heap_flags: int = 0
    _fields_ = [
        ("length", ctypes.c_uint64),
        ("fd", ctypes.c_uint32),
        ("fd_flags", ctypes.c_uint32),
        ("heap_flags", ctypes.c_uint64),
    ]


class Buffer:
    def __init__(self, owner: OwnedFd, size: int) -> None:
        self._owner: OwnedFd = owner
        self._size: int = size

    @classmethod
    def allocate(cls, size: int) -> Result[Buffer]:
        if not 0 < size <= 128 * 1024 * 1024:
            return Rejected(errno.EINVAL, "invalid mock DMA allocation size")
        heap = open_fd(Path("/dev/dma_heap/system"))
        if isinstance(heap, Rejected):
            return heap
        allocation = _Allocation(length=size, fd_flags=os.O_RDWR | os.O_CLOEXEC)
        with heap.value:
            allocated = ioctl(
                heap.value.fd,
                command(3, "H", 0, ctypes.sizeof(_Allocation)),
                allocation,
            )
        if isinstance(allocated, Rejected):
            return allocated
        return Ok(cls(OwnedFd(allocation.fd), size))

    @property
    def fd(self) -> int:
        return self._owner.fd

    @property
    def size(self) -> int:
        return self._size

    def write(self, data: bytes, offset: int = 0) -> Result[None]:
        if offset < 0 or offset + len(data) > self._size:
            return Rejected(errno.EINVAL, "mock DMA write outside allocation")
        started = start_write(self.fd)
        if isinstance(started, Rejected):
            return started
        result: Result[None] = Ok(None)
        try:
            with mmap.mmap(self.fd, self._size, flags=mmap.MAP_SHARED) as mapping:
                mapping[offset : offset + len(data)] = data
        except OSError as error:
            result = Rejected(error.errno or 0, "map mock DMA buffer")
        finally:
            ended = finish_write(self.fd)
        return ended if isinstance(ended, Rejected) else result

    def close(self) -> None:
        self._owner.close()

    def __enter__(self) -> Buffer:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()
