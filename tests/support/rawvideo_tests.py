"""module: rawvideo_tests; responsibility: define bounded process frame digestion.

operations: tests; entities: none; tests: partial pipes, transform failures,
process errors, empty output and deadlines through subprocess interfaces.
"""

import errno
import hashlib
import sys

from tests.support.outcome import Ok, Rejected, Result, accepted
from tests.support.rawvideo import digest


def identity(frame: bytes) -> Result[bytes]:
    return Ok(frame)


def test_digest_frames_span_pipe_writes_and_preserve_order() -> None:
    command = (
        sys.executable,
        "-c",
        "import os; os.write(1,b'ab'); os.write(1,b'cdefghi')",
    )
    result = accepted(digest(command, 3, identity))
    assert result == tuple(
        hashlib.sha256(frame).hexdigest() for frame in (b"abc", b"def", b"ghi")
    )


def test_digest_rejects_truncated_empty_and_failed_process_output() -> None:
    for source in ("print('ab',end='')", "pass", "import sys; sys.exit(7)"):
        result = digest((sys.executable, "-c", source), 3, identity)
        assert isinstance(result, Rejected)


def test_digest_transform_failure_terminates_producer() -> None:
    def reject(frame: bytes) -> Result[bytes]:
        assert frame == b"abc"
        return Rejected(errno.EINVAL, "invalid frame")

    result = digest(
        (sys.executable, "-c", "import os,time; os.write(1,b'abc'); time.sleep(30)"),
        3,
        reject,
    )
    assert isinstance(result, Rejected) and result.operation == "invalid frame"


def test_digest_deadline_terminates_blocked_producer() -> None:
    result = digest(
        (sys.executable, "-c", "import time; time.sleep(30)"), 3, identity, timeout=0.1
    )
    assert isinstance(result, Rejected) and result.code == errno.ETIMEDOUT
