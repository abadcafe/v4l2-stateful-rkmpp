"""module: dmabuf; responsibility: read synchronized DMA-BUF image planes.

operations: read_bytes, i420; entities: none; tests: dmabuf_tests and decode pixel cases.
"""

import errno
import mmap
import sys

from tests.support.dma_sync import finish_read, start_read
from tests.support.outcome import Ok, Rejected, Result


def read_bytes(fd: int, length: int, offset: int = 0) -> Result[bytes]:
    if length <= 0 or offset < 0 or length + offset > sys.maxsize:
        return Rejected(errno.EINVAL, "invalid DMA-BUF read extent")
    start = start_read(fd)
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
        end = finish_read(fd)
    return end if isinstance(end, Rejected) else result


def i420(
    fd: int,
    width: int,
    height: int,
    stride: int,
    rows: int,
    *,
    left: int = 0,
    top: int = 0,
) -> Result[bytes]:
    # A planar 4:2:0 crop must start on a complete chroma sample. Reject
    # invalid layouts before Python slices can silently truncate them.
    if (
        width <= 0
        or height <= 0
        or left < 0
        or top < 0
        or left % 2 != 0
        or top % 2 != 0
        or left + 2 * ((width + 1) // 2) > stride
        or top + height > rows
    ):
        return Rejected(errno.EINVAL, "invalid NV12 crop geometry")
    raw = read_bytes(fd, stride * (rows + (top + height + 1) // 2))
    if isinstance(raw, Rejected):
        return raw
    data = raw.value
    y = b"".join(
        data[(top + row) * stride + left : (top + row) * stride + left + width]
        for row in range(height)
    )
    chroma_width = 2 * ((width + 1) // 2)
    u = b"".join(
        data[
            (rows + top // 2 + row) * stride + left : (rows + top // 2 + row) * stride
            + left
            + chroma_width : 2
        ]
        for row in range((height + 1) // 2)
    )
    v = b"".join(
        data[
            (rows + top // 2 + row) * stride + left + 1 : (rows + top // 2 + row)
            * stride
            + left
            + chroma_width : 2
        ]
        for row in range((height + 1) // 2)
    )
    return Ok(y + u + v)
