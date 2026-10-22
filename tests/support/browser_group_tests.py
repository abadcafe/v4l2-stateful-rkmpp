"""module: browser_group_tests; responsibility: define owned group teardown.

operations: pytest cases; entities: none; tests: descendants after leader exit.
Only launch is substituted; real child groups and pidfds define the observations.
"""

import errno
import os
import select
import signal
import subprocess
import sys
from contextlib import ExitStack
from pathlib import Path
from typing import BinaryIO, Literal

import pytest

from tests.support import browser_launch
from tests.support.browser import Browser
from tests.support.outcome import Ok, Rejected, Result
from tests.support.syscalls import OwnedFd

_DESCENDANT = """import os
import signal
import sys
import time

ready: int = int(sys.argv[1])
retained: int = int(sys.argv[2])
_ = signal.signal(signal.SIGTERM, signal.SIG_IGN)
_ = os.write(ready, str(os.getpid()).encode())
os.close(ready)
time.sleep(60)
os.close(retained)
"""

_LEADER = """import os
import signal
import subprocess
import sys
import time

ready: int = int(sys.argv[2])
retained: int = int(sys.argv[3])
phase: str = sys.argv[4]
assert phase in ("exited", "stubborn")
if phase == "stubborn":
    _ = signal.signal(signal.SIGTERM, signal.SIG_IGN)
child: subprocess.Popen[bytes] = subprocess.Popen(
    [sys.executable, sys.argv[1], str(ready), str(retained)],
    pass_fds=(ready, retained),
)
if phase == "exited":
    os._exit(0)
time.sleep(60)
"""


@pytest.mark.parametrize("phase", ["exited", "stubborn"])
def test_open_failed_start_terminates_owned_group(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    phase: Literal["exited", "stubborn"],
) -> None:
    leader_script = tmp_path / "leader.py"
    descendant_script = tmp_path / "descendant.py"
    _ = leader_script.write_text(_LEADER)
    _ = descendant_script.write_text(_DESCENDANT)
    roots: list[subprocess.Popen[bytes]] = []
    descendants: list[OwnedFd] = []
    with ExitStack() as resources:
        reader_fd, writer_fd = os.pipe2(os.O_CLOEXEC)
        retained = resources.enter_context(OwnedFd(reader_fd))
        writer = resources.enter_context(OwnedFd(writer_fd))
        ready_reader_fd, ready_writer_fd = os.pipe2(os.O_CLOEXEC)
        ready = resources.enter_context(OwnedFd(ready_reader_fd))
        ready_writer = resources.enter_context(OwnedFd(ready_writer_fd))
        unrelated: subprocess.Popen[bytes] = subprocess.Popen(
            ["/usr/bin/sleep", "60"], start_new_session=True
        )

        def launch(
            profile: Path, log: BinaryIO, flags: tuple[str, ...], executable: Path
        ) -> Result[subprocess.Popen[bytes]]:
            assert (
                profile.parent == tmp_path / "browser"
                and not flags
                and executable.is_file()
            )
            root: subprocess.Popen[bytes] = subprocess.Popen(
                [
                    sys.executable,
                    str(leader_script),
                    str(descendant_script),
                    str(ready_writer.fd),
                    str(writer.fd),
                    phase,
                ],
                pass_fds=(ready_writer.fd, writer.fd),
                stdout=log,
                stderr=log,
                start_new_session=True,
            )
            roots.append(root)
            writer.close()
            ready_writer.close()
            assert select.select([ready.fd], [], [], 5)[0]
            pid = int(os.read(ready.fd, 64))
            descendants.append(resources.enter_context(OwnedFd(os.pidfd_open(pid))))
            if phase == "exited":
                status = os.waitid(os.P_PID, root.pid, os.WEXITED | os.WNOWAIT)
                assert status is not None and status.si_code == os.CLD_EXITED
            else:
                assert (
                    os.waitid(os.P_PID, root.pid, os.WEXITED | os.WNOWAIT | os.WNOHANG)
                    is None
                )
            return Ok(root)

        monkeypatch.setattr(browser_launch, "launch", launch)
        try:
            result = Browser.open(tmp_path / "browser", (), (), startup_timeout=0.1)
            assert isinstance(result, Rejected) and result.code == errno.ETIMEDOUT
            if phase == "stubborn":
                assert "owned browser required SIGKILL" in result.operation
            assert len(roots) == len(descendants) == 1
            # pidfd readiness is terminal status of the original descendant,
            # independent of numeric PID reuse and who reaps the orphan.
            assert select.select([descendants[0].fd], [], [], 1)[0], (
                "Browser cleanup left a live descendant after its leader exited"
            )
            assert select.select([retained.fd], [], [], 1)[0]
            assert os.read(retained.fd, 1) == b""
            assert unrelated.poll() is None
            expected = 0 if phase == "exited" else -signal.SIGKILL
            assert roots[0].wait(timeout=1) == expected
        finally:
            for descriptor in descendants:
                if not select.select([descriptor.fd], [], [], 0)[0]:
                    signal.pidfd_send_signal(descriptor.fd, signal.SIGKILL)
                assert select.select([descriptor.fd], [], [], 5)[0]
            for root in roots:
                if root.returncode is None:
                    try:
                        os.killpg(root.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                _ = root.wait(timeout=5)
            unrelated.terminate()
            _ = unrelated.wait(timeout=5)
