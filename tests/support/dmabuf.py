"""module: dmabuf; responsibility: read synchronized DMA-BUF image planes.

operations: read_bytes, i420; entities: none; tests: dmabuf_tests and decode pixel cases.
"""

import ctypes
import mmap
from typing import final

from tests.support.outcome import Ok, Rejected, Result
from tests.support.syscalls import command, ioctl


@final
class _Sync(ctypes.Structure):
    flags: int = 0
    _fields_ = [("flags", ctypes.c_uint64)]


def read_bytes(fd: int, length: int, offset: int = 0) -> Result[bytes]:
    start = ioctl(fd, command(1, "b", 0, 8), _Sync(flags=1))
    if isinstance(start, Rejected):
        return start
    result: Result[bytes]
    try:
        with mmap.mmap(
            fd, length + offset, flags=mmap.MAP_SHARED, prot=mmap.PROT_READ
        ) as mapping:
            result = Ok(mapping[offset : offset + length])
    except OSError as error:
        result = Rejected(error.errno or 0, "mmap DMA-BUF")
    finally:
        end = ioctl(fd, command(1, "b", 0, 8), _Sync(flags=5))
    return end if isinstance(end, Rejected) else result


def i420(fd: int, width: int, height: int, stride: int, rows: int) -> Result[bytes]:
    raw = read_bytes(fd, stride * (rows + height // 2))
    if isinstance(raw, Rejected):
        return raw
    data = raw.value
    y = b"".join(data[row * stride : row * stride + width] for row in range(height))
    u = b"".join(
        data[(rows + row) * stride : (rows + row) * stride + width : 2]
        for row in range(height // 2)
    )
    v = b"".join(
        data[(rows + row) * stride + 1 : (rows + row) * stride + width : 2]
        for row in range(height // 2)
    )
    return Ok(y + u + v)
