"""module: tests.support.dmabuf_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: _Allocate, _Sync; tests:
    test_read_bytes_and_i420_reject_non_dma_file
    test_i420_strips_padding_and_deinterleaves_chroma


"""

import ctypes
import errno
import mmap
import os
from pathlib import Path
from typing import final

from tests.support.dmabuf import i420, read_bytes
from tests.support.outcome import Rejected, accepted
from tests.support.syscalls import OwnedFd, command, ioctl, open_fd


@final
class _Allocate(ctypes.Structure):
    fd: int = 0
    _fields_ = [
        ("size", ctypes.c_uint64),
        ("fd", ctypes.c_uint32),
        ("flags", ctypes.c_uint32),
        ("heap_flags", ctypes.c_uint64),
    ]


@final
class _Sync(ctypes.Structure):
    _fields_ = [("flags", ctypes.c_uint64)]


def test_read_bytes_and_i420_reject_non_dma_file() -> None:
    with accepted(open_fd(Path("/dev/null"))) as owner:
        for result in (read_bytes(owner.fd, 4096), i420(owner.fd, 4, 2, 8, 4)):
            assert isinstance(result, Rejected) and result.code == errno.ENOTTY


def test_i420_strips_padding_and_deinterleaves_chroma() -> None:
    with accepted(open_fd(Path("/dev/dma_heap/system"))) as heap:
        allocation = _Allocate(size=4096, flags=os.O_RDWR | os.O_CLOEXEC)
        _ = accepted(ioctl(heap.fd, command(3, "H", 0, 24), allocation))
    with OwnedFd(allocation.fd) as owner:
        _ = accepted(ioctl(owner.fd, command(1, "b", 0, 8), _Sync(flags=3)))
        with mmap.mmap(owner.fd, 4096) as mapping:
            mapping[:] = bytes([165]) * 4096
            mapping[:4] = bytes([0, 1, 2, 3])
            mapping[8:12] = bytes([4, 5, 6, 7])
            mapping[32:36] = bytes([10, 20, 11, 21])
        _ = accepted(ioctl(owner.fd, command(1, "b", 0, 8), _Sync(flags=7)))
        assert accepted(read_bytes(owner.fd, 4, 32)) == bytes([10, 20, 11, 21])
        assert accepted(i420(owner.fd, 4, 2, 8, 4)) == bytes(
            [0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 20, 21]
        )
