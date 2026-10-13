"""module: video_formats; responsibility: negotiate V4L2 image layouts.

operations: get_format, set_format, enumerate_formats, selection;
entities: VideoFormat; tests: video_formats_tests, kernel/test_formats.
"""

import ctypes
import errno
import struct

from pydantic import BaseModel

from tests.support.format_abi import Format, FormatDescription, Selection
from tests.support.outcome import Ok, Rejected, Result
from tests.support.syscalls import command, ioctl


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


def _describe(payload: Format) -> VideoFormat:
    data = bytes(payload.fmt.data)
    width, height, fourcc, _, colorspace = struct.unpack_from("<5I", data)
    planes = data[180]
    return VideoFormat(
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


def get_format(fd: int, kind: int) -> Result[VideoFormat]:
    payload = Format(kind=kind)
    result = ioctl(fd, command(3, "V", 4, ctypes.sizeof(payload)), payload)
    return Ok(_describe(payload)) if isinstance(result, Ok) else result


def set_format(
    fd: int,
    kind: int,
    fourcc: int,
    *,
    width: int = 0,
    height: int = 0,
    size: int = 16 * 1024 * 1024,
) -> Result[VideoFormat]:
    payload = Format(kind=kind)
    data = bytearray(200)
    struct.pack_into("<5I", data, 0, width, height, fourcc, 0, 0)
    struct.pack_into("<I", data, 20, size)
    data[180] = 1
    payload.fmt.data[:] = data
    result = ioctl(fd, command(3, "V", 5, ctypes.sizeof(payload)), payload)
    return Ok(_describe(payload)) if isinstance(result, Ok) else result


def enumerate_formats(fd: int, kind: int) -> Result[list[int]]:
    formats: list[int] = []
    while True:
        payload = FormatDescription(index=len(formats), kind=kind)
        result = ioctl(fd, command(3, "V", 2, ctypes.sizeof(payload)), payload)
        if isinstance(result, Rejected):
            return Ok(formats) if result.code == errno.EINVAL else result
        formats.append(payload.pixelformat)


def selection(fd: int, kind: int) -> Result[tuple[int, int]]:
    payload = Selection(kind=kind, target=0x100)
    result = ioctl(fd, command(3, "V", 94, ctypes.sizeof(payload)), payload)
    return (
        Ok((payload.rect.width, payload.rect.height))
        if isinstance(result, Ok)
        else result
    )
