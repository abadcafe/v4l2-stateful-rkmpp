"""module: reference; responsibility: produce independent decoded pixel expectations.

operations: quantize, frame_hashes, frame_positions; entities: _Position, _Positions;
tests: reference_tests and all codec pixel acceptance cases.
"""

import array
import errno
import hashlib
import subprocess
import sys
from pathlib import Path

from pydantic import BaseModel, ValidationError

from tests.support.outcome import Ok, Rejected, Result


class _Position(BaseModel):
    pkt_pos: int


class _Positions(BaseModel):
    frames: tuple[_Position, ...]


def quantize(raw: bytes, width: int, height: int) -> Result[bytes]:
    samples: array.array[int] = array.array("H")
    frame_samples = width * height + 2 * ((width + 1) // 2) * ((height + 1) // 2)
    if not raw or width <= 0 or height <= 0 or len(raw) % (frame_samples * 2):
        return Rejected(errno.EINVAL, "invalid planar 10-bit reference extent")
    samples.frombytes(raw)
    if sys.byteorder != "little":
        samples.byteswap()
    result = bytearray()
    offset = 0
    while offset < len(samples):
        for plane_width, plane_height in (
            (width, height),
            ((width + 1) // 2, (height + 1) // 2),
            ((width + 1) // 2, (height + 1) // 2),
        ):
            for row in range(plane_height):
                for column in range(plane_width):
                    threshold = ((0, 2), (3, 1))[row % 2][column % 2]
                    result.append(min(255, (samples[offset] + threshold) // 4))
                    offset += 1
    return Ok(bytes(result))


def frame_hashes(
    path: Path, depth: int, width: int, height: int
) -> Result[tuple[str, ...]]:
    assert depth in (8, 10) and width > 0 and height > 0
    try:
        result = subprocess.run(
            [
                "ffmpeg",
                "-v",
                "error",
                "-i",
                str(path),
                "-pix_fmt",
                "yuv420p10le" if depth == 10 else "yuv420p",
                "-f",
                "rawvideo",
                "-",
            ],
            capture_output=True,
            check=False,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        return Rejected(errno.ETIMEDOUT, "FFmpeg reference deadline")
    except OSError as error:
        return Rejected(error.errno or 0, "FFmpeg reference process")
    if result.returncode:
        return Rejected(
            result.returncode,
            "ffmpeg reference: " + result.stderr.decode(errors="replace"),
        )
    raw = result.stdout
    if depth == 10:
        converted = quantize(raw, width, height)
        if isinstance(converted, Rejected):
            return converted
        raw = converted.value
    size = width * height + 2 * ((width + 1) // 2) * ((height + 1) // 2)
    if not raw or len(raw) % size:
        return Rejected(errno.EINVAL, "reference frame dimensions do not match")
    frames = memoryview(raw)
    return Ok(
        tuple(
            hashlib.sha256(frames[offset : offset + size]).hexdigest()
            for offset in range(0, len(raw), size)
        )
    )


def frame_positions(path: Path) -> Result[tuple[int, ...]]:
    """Return input byte provenance in independent software display order."""
    try:
        probe = subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-show_frames",
                "-show_entries",
                "frame=pkt_pos",
                "-of",
                "json",
                str(path),
            ],
            capture_output=True,
            check=False,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return Rejected(errno.EIO, f"probe frame provenance: {error}")
    if probe.returncode:
        return Rejected(errno.EINVAL, probe.stderr.decode(errors="replace"))
    try:
        frames = _Positions.model_validate_json(probe.stdout).frames
    except ValidationError as error:
        return Rejected(errno.EINVAL, f"invalid frame provenance: {error}")
    return Ok(tuple(frame.pkt_pos for frame in frames))
