"""module: pixels; responsibility: inspect RGB screenshot regions.

operations: decode_png, crop_rgb; entities: Rgb; tests: pixels_tests, browser pixels.
"""

import errno
import struct
import subprocess

from pydantic import BaseModel

from tests.support.outcome import Ok, Rejected, Result


class Rgb(BaseModel):
    width: int
    height: int
    pixels: bytes


def decode_png(png: bytes) -> Result[Rgb]:
    if len(png) < 33 or png[:8] != b"\x89PNG\r\n\x1a\n" or png[12:16] != b"IHDR":
        return Rejected(errno.EINVAL, "invalid PNG header")
    width, height = struct.unpack_from(">2I", png, 16)
    if not 0 < width <= 16384 or not 0 < height <= 16384:
        return Rejected(errno.EINVAL, "PNG extent outside screenshot limits")
    try:
        result = subprocess.run(
            [
                "ffmpeg",
                "-v",
                "error",
                "-f",
                "image2pipe",
                "-i",
                "pipe:0",
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "pipe:1",
            ],
            input=png,
            capture_output=True,
            timeout=15,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return Rejected(errno.ETIMEDOUT, "FFmpeg PNG deadline")
    except OSError as error:
        return Rejected(error.errno or 0, "FFmpeg PNG process")
    if result.returncode or len(result.stdout) != width * height * 3:
        return Rejected(errno.EBADMSG, "FFmpeg PNG decode failed")
    return Ok(Rgb(width=width, height=height, pixels=result.stdout))


def crop_rgb(image: Rgb, x: int, y: int, width: int, height: int) -> Result[bytes]:
    if (
        x < 0
        or y < 0
        or width <= 0
        or height <= 0
        or x + width > image.width
        or y + height > image.height
        or len(image.pixels) != image.width * image.height * 3
    ):
        return Rejected(errno.EINVAL, "RGB crop outside image")
    return Ok(
        b"".join(
            image.pixels[
                (row * image.width + x) * 3 : (row * image.width + x + width) * 3
            ]
            for row in range(y, y + height)
        )
    )
