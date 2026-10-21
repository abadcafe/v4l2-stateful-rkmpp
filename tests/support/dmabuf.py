"""module: dmabuf; responsibility: read synchronized DMA-BUF image planes.

operations: read_bytes, i420; entities: none; tests: dmabuf_tests and decode pixel cases.
"""

import errno
import mmap
import sys
from collections.abc import Callable

import numpy as np

from tests.support.dma_sync import finish_read, start_read
from tests.support.outcome import Ok, Rejected, Result


def read_bytes(fd: int, length: int, offset: int = 0) -> Result[bytes]:
    if length <= 0 or offset < 0 or length + offset > sys.maxsize:
        return Rejected(errno.EINVAL, "invalid DMA-BUF read extent")

    def copy(mapping: mmap.mmap) -> bytes:
        return mapping[offset : offset + length]

    return _read(fd, length + offset, copy)


def _read(fd: int, length: int, extract: Callable[[mmap.mmap], bytes]) -> Result[bytes]:
    if not 0 < length <= sys.maxsize:
        return Rejected(errno.EINVAL, "invalid DMA-BUF read extent")
    start = start_read(fd)
    if isinstance(start, Rejected):
        return start
    result: Result[bytes]
    try:
        with mmap.mmap(
            fd, length, flags=mmap.MAP_SHARED, prot=mmap.PROT_READ
        ) as mapping:
            result = Ok(extract(mapping))
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

    def extract(mapping: mmap.mmap) -> bytes:
        # Strided arrays borrow the synchronized mapping. Only the compact
        # destination is allocated; padding and intermediate Y/U/V planes
        # are never copied into Python buffers.
        chroma_width, chroma_height = (width + 1) // 2, (height + 1) // 2
        luma_size, chroma_size = width * height, chroma_width * chroma_height
        pixels: np.ndarray[tuple[int, ...], np.dtype[np.uint8]] = np.empty(
            luma_size + 2 * chroma_size, dtype=np.uint8
        )
        luma: np.ndarray[tuple[int, ...], np.dtype[np.uint8]] = np.ndarray(
            (height, width),
            dtype=np.uint8,
            buffer=mapping,
            offset=top * stride + left,
            strides=(stride, 1),
        )
        chroma: np.ndarray[tuple[int, ...], np.dtype[np.uint8]] = np.ndarray(
            (chroma_height, chroma_width, 2),
            dtype=np.uint8,
            buffer=mapping,
            offset=(rows + top // 2) * stride + left,
            strides=(stride, 2, 1),
        )
        pixels[:luma_size].reshape(height, width)[:] = luma
        pixels[luma_size : luma_size + chroma_size].reshape(
            chroma_height, chroma_width
        )[:] = chroma[:, :, 0]
        pixels[luma_size + chroma_size :].reshape(chroma_height, chroma_width)[:] = (
            chroma[:, :, 1]
        )
        return pixels.tobytes()

    return _read(fd, stride * (rows + (top + height + 1) // 2), extract)
