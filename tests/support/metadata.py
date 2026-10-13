"""module: metadata; responsibility: inspect source video metadata independently.

operations: inspect; entities: VideoInfo; tests: metadata_tests.
"""

import errno
import subprocess
from pathlib import Path

from pydantic import BaseModel, ValidationError

from tests.support.outcome import Ok, Rejected, Result


class VideoInfo(BaseModel):
    pix_fmt: str
    width: int
    height: int
    codec_name: str
    profile: str | None = None
    color_primaries: str | None = None
    color_transfer: str | None = None
    color_space: str | None = None
    color_range: str | None = None


class _Probe(BaseModel):
    streams: list[VideoInfo]


def inspect(path: Path) -> Result[VideoInfo]:
    try:
        result = subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_streams",
                "-of",
                "json",
                str(path),
            ],
            capture_output=True,
            text=True,
            check=False,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        return Rejected(errno.ETIMEDOUT, "ffprobe video metadata")
    except OSError as error:
        return Rejected(error.errno or 0, "ffprobe process")
    if result.returncode:
        return Rejected(errno.EBADMSG, result.stderr)
    try:
        probe = _Probe.model_validate_json(result.stdout, strict=True)
    except ValidationError as error:
        return Rejected(errno.EBADMSG, str(error))
    return (
        Ok(probe.streams[0])
        if len(probe.streams) == 1
        else Rejected(errno.ENODATA, "missing video stream")
    )
