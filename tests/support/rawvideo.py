"""module: rawvideo; responsibility: digest bounded raw frames from a process.

operations: digest; entities: Transform; tests: rawvideo_tests.
Pipe framing, process deadlines and cleanup are hidden from pixel policy.
"""

import errno
import hashlib
import math
import os
import selectors
import subprocess
import tempfile
import time
from collections.abc import Callable
from typing import IO

from tests.support.outcome import Ok, Rejected, Result

type Transform = Callable[[bytes], Result[bytes]]


def digest(
    command: tuple[str, ...],
    frame_size: int,
    transform: Transform,
    *,
    timeout: float = 30,
) -> Result[tuple[str, ...]]:
    if (
        not command
        or not 0 < frame_size <= 128 * 1024 * 1024
        or not math.isfinite(timeout)
        or timeout <= 0
    ):
        return Rejected(errno.EINVAL, "invalid raw frame process contract")
    deadline = time.monotonic() + timeout
    try:
        with tempfile.TemporaryFile() as errors:
            child: subprocess.Popen[bytes] = subprocess.Popen(
                command, stdout=subprocess.PIPE, stderr=errors
            )
            assert child.stdout is not None
            result: Result[tuple[str, ...]]
            try:
                result = _consume(child.stdout, frame_size, transform, deadline)
                if isinstance(result, Ok):
                    try:
                        code = child.wait(timeout=max(0, deadline - time.monotonic()))
                    except subprocess.TimeoutExpired:
                        result = Rejected(errno.ETIMEDOUT, "raw frame process deadline")
                    else:
                        if code:
                            _ = errors.seek(0)
                            result = Rejected(
                                errno.EBADMSG,
                                errors.read(65536).decode(errors="replace"),
                            )
            finally:
                if child.poll() is None:
                    child.kill()
                    _ = child.wait()
                child.stdout.close()
            return result
    except OSError as error:
        return Rejected(error.errno or 0, f"raw frame process: {error}")


def _consume(
    source: IO[bytes],
    frame_size: int,
    transform: Transform,
    deadline: float,
) -> Result[tuple[str, ...]]:
    os.set_blocking(source.fileno(), False)
    pending = bytearray()
    hashes: list[str] = []
    with selectors.DefaultSelector() as ready:
        _ = ready.register(source, selectors.EVENT_READ)
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not ready.select(remaining):
                return Rejected(errno.ETIMEDOUT, "raw frame process deadline")
            try:
                block = os.read(
                    source.fileno(), min(frame_size - len(pending), 1024 * 1024)
                )
            except OSError as error:
                if error.errno in (errno.EINTR, errno.EAGAIN):
                    continue
                return Rejected(error.errno or 0, "read raw frame pipe")
            if not block:
                break
            pending.extend(block)
            if len(pending) == frame_size:
                frame = transform(bytes(pending))
                if isinstance(frame, Rejected):
                    return frame
                hashes.append(hashlib.sha256(frame.value).hexdigest())
                pending.clear()
    if pending or not hashes:
        return Rejected(errno.EINVAL, "empty or truncated raw frame output")
    return Ok(tuple(hashes))
