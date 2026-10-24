"""module: video_formats; responsibility: negotiate V4L2 image layouts.

operations: get_format, set_format, enumerate_formats, selection;
entities: VideoFormat; tests: video_formats_tests, kernel/test_formats.
"""

import ctypes
import errno
import struct

from pydantic import BaseModel

from tests.support import syscalls
from tests.support.format_abi import Format, FormatDescription, Selection
from tests.support.outcome import Ok, Rejected, Result

_U32_MAX = (1 << 32) - 1
_VIDEO_MAX_PLANES = 8


class VideoFormat(BaseModel):
    width: int
    height: int
    fourcc: int
    planes: int
    sizes: tuple[int, ...]
    strides: tuple[int, ...]
    colorspace: int
    ycbcr: int
    quantization: int
    transfer: int


def _describe(payload: Format) -> Result[VideoFormat]:
    data = bytes(payload.fmt.data)
    width, height, fourcc, _, colorspace = struct.unpack_from("<5I", data)
    planes = data[180]
    # v4l2_pix_format_mplane contains exactly VIDEO_MAX_PLANES entries.
    # Validate the driver's count before indexing its fixed ABI storage.
    if not 1 <= planes <= _VIDEO_MAX_PLANES:
        return Rejected(errno.EPROTO, "driver returned an invalid video plane count")
    return Ok(
        VideoFormat(
            width=width,
            height=height,
            fourcc=fourcc,
            planes=planes,
            sizes=tuple(
                struct.unpack_from("<I", data, 20 + 20 * i)[0] for i in range(planes)
            ),
            strides=tuple(
                struct.unpack_from("<I", data, 24 + 20 * i)[0] for i in range(planes)
            ),
            colorspace=colorspace,
            ycbcr=data[182],
            quantization=data[183],
            transfer=data[184],
        )
    )


def get_format(fd: int, kind: int) -> Result[VideoFormat]:
    if not 0 <= kind <= _U32_MAX:
        return Rejected(errno.EINVAL, "queue type outside unsigned 32-bit ABI")
    payload = Format(kind=kind)
    result = syscalls.ioctl(
        fd, syscalls.command(3, "V", 4, ctypes.sizeof(payload)), payload
    )
    return _describe(payload) if isinstance(result, Ok) else result


def set_format(
    fd: int,
    kind: int,
    fourcc: int,
    *,
    width: int = 0,
    height: int = 0,
    size: int = 16 * 1024 * 1024,
) -> Result[VideoFormat]:
    if any(not 0 <= value <= _U32_MAX for value in (kind, fourcc, width, height, size)):
        return Rejected(errno.EINVAL, "format value outside unsigned 32-bit ABI")
    payload = Format(kind=kind)
    data = bytearray(200)
    struct.pack_into("<5I", data, 0, width, height, fourcc, 0, 0)
    struct.pack_into("<I", data, 20, size)
    data[180] = 1
    payload.fmt.data[:] = data
    result = syscalls.ioctl(
        fd, syscalls.command(3, "V", 5, ctypes.sizeof(payload)), payload
    )
    return _describe(payload) if isinstance(result, Ok) else result


def enumerate_formats(fd: int, kind: int) -> Result[list[int]]:
    if not 0 <= kind <= _U32_MAX:
        return Rejected(errno.EINVAL, "queue type outside unsigned 32-bit ABI")
    formats: list[int] = []
    while True:
        payload = FormatDescription(index=len(formats), kind=kind)
        result = syscalls.ioctl(
            fd, syscalls.command(3, "V", 2, ctypes.sizeof(payload)), payload
        )
        if isinstance(result, Rejected):
            return Ok(formats) if result.code == errno.EINVAL else result
        formats.append(payload.pixelformat)


def selection(fd: int, kind: int) -> Result[tuple[int, int]]:
    if not 0 <= kind <= _U32_MAX:
        return Rejected(errno.EINVAL, "queue type outside unsigned 32-bit ABI")
    payload = Selection(kind=kind, target=0x100)
    result = syscalls.ioctl(
        fd, syscalls.command(3, "V", 94, ctypes.sizeof(payload)), payload
    )
    return (
        Ok((payload.rect.width, payload.rect.height))
        if isinstance(result, Ok)
        else result
    )
