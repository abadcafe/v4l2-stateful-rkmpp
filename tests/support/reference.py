"""module: reference; responsibility: produce independent decoded pixel expectations.

operations: quantize, frame_hashes, frame_positions; entities: _Position, _Positions;
tests: reference_tests and all codec pixel acceptance cases.
"""

import errno
import subprocess
from pathlib import Path

import numpy as np
from pydantic import BaseModel, ValidationError

from tests.support.outcome import Ok, Rejected, Result
from tests.support.rawvideo import digest


class _Position(BaseModel):
    pkt_pos: int


class _Positions(BaseModel):
    frames: tuple[_Position, ...]


def quantize(raw: bytes, width: int, height: int) -> Result[bytes]:
    frame_samples = width * height + 2 * ((width + 1) // 2) * ((height + 1) // 2)
    if not raw or width <= 0 or height <= 0 or len(raw) % (frame_samples * 2):
        return Rejected(errno.EINVAL, "invalid planar 10-bit reference extent")
    samples: np.ndarray[tuple[int, ...], np.dtype[np.uint16]] = np.frombuffer(
        raw, dtype=np.dtype("<u2")
    )
    result: np.ndarray[tuple[int, ...], np.dtype[np.uint8]] = np.empty(
        len(samples), dtype=np.uint8
    )
    offset = 0
    while offset < len(samples):
        for plane_width, plane_height in (
            (width, height),
            ((width + 1) // 2, (height + 1) // 2),
            ((width + 1) // 2, (height + 1) // 2),
        ):
            end = offset + plane_width * plane_height
            plane: np.ndarray[tuple[int, ...], np.dtype[np.uint16]] = samples[
                offset:end
            ].reshape(plane_height, plane_width)
            values: np.ndarray[tuple[int, ...], np.dtype[np.uint16]] = np.minimum(
                plane, np.uint16(1023)
            )
            values[::2, 1::2] += np.uint16(2)
            values[1::2, ::2] += np.uint16(3)
            values[1::2, 1::2] += np.uint16(1)
            _ = np.right_shift(values, 2, out=values)
            _ = np.minimum(values, np.uint16(255), out=values)
            result[offset:end] = values.reshape(-1)
            offset = end
    return Ok(result.tobytes())


def frame_hashes(
    path: Path, depth: int, width: int, height: int
) -> Result[tuple[str, ...]]:
    assert depth in (8, 10) and width > 0 and height > 0
    command = (
        "ffmpeg",
        "-v",
        "error",
        "-i",
        str(path),
        "-pix_fmt",
        "yuv420p10le" if depth == 10 else "yuv420p",
        # A reference contains one output per decoded picture. FFmpeg's
        # automatic rawvideo sync duplicates/drops pictures at VFR timestamps.
        "-fps_mode",
        "passthrough",
        "-f",
        "rawvideo",
        "-",
    )
    size = width * height + 2 * ((width + 1) // 2) * ((height + 1) // 2)

    def convert(frame: bytes) -> Result[bytes]:
        return quantize(frame, width, height) if depth == 10 else Ok(frame)

    return digest(command, size * (2 if depth == 10 else 1), convert)


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
