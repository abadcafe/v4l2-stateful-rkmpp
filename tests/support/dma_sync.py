"""module: dma_sync; responsibility: synchronize DMA-BUF CPU access.

operations: start_read, finish_read, start_write, finish_write; entities: _Sync;
tests: dma_sync_tests, dma_heap_tests, dmabuf_tests. Linux requires restarting
DMA_BUF_IOCTL_SYNC after either EINTR or EAGAIN, including at access completion.
"""

import ctypes
import errno
from typing import final

from tests.support import syscalls
from tests.support.outcome import Ok, Result


@final
class _Sync(ctypes.Structure):
    flags: int = 0
    _fields_ = [("flags", ctypes.c_uint64)]


def _synchronize(fd: int, flags: int) -> Result[None]:
    while True:
        result = syscalls.ioctl(fd, syscalls.command(1, "b", 0, 8), _Sync(flags=flags))
        if isinstance(result, Ok):
            return Ok(None)
        if result.code not in (errno.EINTR, errno.EAGAIN):
            return result


def start_read(fd: int) -> Result[None]:
    return _synchronize(fd, 1)


def finish_read(fd: int) -> Result[None]:
    return _synchronize(fd, 5)


def start_write(fd: int) -> Result[None]:
    return _synchronize(fd, 3)


def finish_write(fd: int) -> Result[None]:
    return _synchronize(fd, 7)
