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

import pytest

from tests.support.dma_heap import Buffer
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


@pytest.mark.parametrize(
    "length,offset", [(-1, 0), (0, 0), (1, -1), (4097, 0), (1, 4096), (1 << 128, 0)]
)
def test_read_bytes_rejects_invalid_extent_without_mapping(
    length: int, offset: int
) -> None:
    with accepted(Buffer.allocate(4096)) as buffer:
        result = read_bytes(buffer.fd, length, offset)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
        accepted(buffer.write(b"still accessible"))


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


@pytest.mark.parametrize(
    "width,height,stride,rows,left,top",
    [
        (0, 2, 8, 4, 0, 0),
        (4, 0, 8, 4, 0, 0),
        (4, 2, 3, 4, 0, 0),
        (4, 2, 8, 1, 0, 0),
        (4, 2, 8, 4, -2, 0),
        (4, 2, 8, 4, 0, -2),
        (4, 2, 8, 4, 6, 0),
        (4, 2, 8, 4, 0, 4),
        (4, 2, 8, 4, 1, 0),
        (4, 2, 8, 4, 0, 1),
    ],
)
def test_i420_rejects_invalid_or_chroma_unaligned_geometry(
    width: int, height: int, stride: int, rows: int, left: int, top: int
) -> None:
    with accepted(Buffer.allocate(4096)) as buffer:
        result = i420(buffer.fd, width, height, stride, rows, left=left, top=top)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL


@pytest.mark.parametrize(
    "width,height,left,top,stride,rows",
    [(1, 1, 0, 0, 4, 2), (3, 3, 2, 2, 8, 6), (7, 5, 0, 0, 8, 6)],
)
def test_i420_odd_crop_preserves_planar_order_and_owns_returned_pixels(
    width: int, height: int, left: int, top: int, stride: int, rows: int
) -> None:
    raw = bytes((index * 13 + 7) % 256 for index in range(4096))
    expected = bytes(
        raw[(top + row) * stride + left + column]
        for row in range(height)
        for column in range(width)
    ) + b"".join(
        bytes(
            raw[(rows + top // 2 + row) * stride + left + 2 * column + plane]
            for row in range((height + 1) // 2)
            for column in range((width + 1) // 2)
        )
        for plane in (0, 1)
    )
    with accepted(Buffer.allocate(4096)) as buffer:
        accepted(buffer.write(raw))
        pixels = accepted(
            i420(buffer.fd, width, height, stride, rows, left=left, top=top)
        )
        accepted(buffer.write(bytes(4096)))
    assert pixels == expected
