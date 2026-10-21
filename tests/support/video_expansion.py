"""module: video_expansion; responsibility: expand encoded cycles without re-encoding.

operations: expand; entities: none; tests: video_expansion_tests.
The caller owns the new destination, including partial output on failure/cancel.
"""

import errno
import subprocess
from pathlib import Path

from tests.support.outcome import Ok, Rejected, Result


def expand(source: Path, destination: Path, frames: int) -> Result[None]:
    if destination.exists():
        return Rejected(errno.EEXIST, "measurement destination already exists")
    if frames <= 0:
        return Rejected(errno.EINVAL, "expansion requires a positive frame count")
    try:
        result = subprocess.run(
            [
                "ffmpeg",
                "-hide_banner",
                "-v",
                "error",
                "-nostdin",
                "-n",
                "-stream_loop",
                "-1",
                "-i",
                str(source),
                "-map",
                "0:v:0",
                "-c",
                "copy",
                "-frames:v",
                str(frames),
                str(destination),
            ],
            capture_output=True,
            text=True,
            check=False,
            timeout=60,
        )
    except subprocess.TimeoutExpired:
        return Rejected(errno.ETIMEDOUT, "expand measurement video")
    except OSError as error:
        return Rejected(error.errno or 0, f"expand measurement video: {error}")
    if result.returncode:
        return Rejected(errno.EBADMSG, f"expand measurement video: {result.stderr}")
    return Ok(None)
