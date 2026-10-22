"""module: service_process_tests; responsibility: define daemon lifetime ownership.

operations: pytest cases; entities: none; tests: borrowed ownership and startup failure.
Real successful starts are exercised by the browser acceptance fixture.
"""

import errno
import signal
import subprocess
import time
from collections.abc import Sequence
from pathlib import Path
from typing import BinaryIO, override

import pytest

from tests.support.diagnostics import query
from tests.support.mock_process import Process
from tests.support.outcome import Rejected, accepted
from tests.support.service_process import Lease
from tests.support.service_runtime import Running


def test_start_failure_waits_for_terminal_status_after_forced_cleanup(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    processes: list[subprocess.Popen[bytes]] = []
    logs: list[BinaryIO] = []
    observed_terminal: list[bool] = []

    class DelayedObservation(subprocess.Popen[bytes]):
        @override
        def wait(self, timeout: float | None = None) -> int:
            # A deadline expiring is an observation failure, including after
            # SIGKILL. Only the final actual wait establishes termination.
            if timeout is not None:
                raise subprocess.TimeoutExpired(self.args, timeout)
            status = super().wait()
            observed_terminal.append(not logs[0].closed)
            return status

    def launch(
        args: Sequence[str], *, stdout: BinaryIO, stderr: BinaryIO
    ) -> subprocess.Popen[bytes]:
        assert args[1] == "serve" and stdout is stderr
        logs.append(stdout)
        process = DelayedObservation(
            ["/usr/bin/env", "--ignore-signal=TERM", "/usr/bin/sleep", "60"],
            stdout=stdout,
            stderr=stderr,
        )
        processes.append(process)
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            fields = dict(
                line.split(":", 1)
                for line in Path(f"/proc/{process.pid}/status").read_text().splitlines()
                if ":" in line
            )
            if int(fields["SigIgn"].strip(), 16) & (1 << (signal.SIGTERM - 1)):
                return process
            time.sleep(0.001)
        assert False, "owned fixture did not install SIGTERM disposition"

    monkeypatch.setattr(subprocess, "Popen", launch)
    try:
        result = Lease.start(
            Path("/unused/service"),
            Path("/dev/null"),
            tmp_path / "absent-endpoint",
            tmp_path / "service.log",
            startup_timeout=0.02,
        )
        assert isinstance(result, Rejected) and result.code == errno.EIO
        assert "failed to become ready" in result.operation
        assert "cleanup: owned service required SIGKILL" in result.operation
        assert observed_terminal == [True]
        assert processes[0].returncode == -signal.SIGKILL
        assert logs[0].closed
    finally:
        for process in processes:
            if process.poll() is None:
                process.kill()
            _ = process.wait()


@pytest.mark.parametrize(
    "timeout", [0.0, -1.0, float("nan"), float("inf"), -float("inf")]
)
def test_start_invalid_deadline_rejects_before_opening_log(
    tmp_path: Path, timeout: float
) -> None:
    log = tmp_path / "service.log"
    result = Lease.start(
        tmp_path / "missing",
        Path("/dev/null"),
        tmp_path / "status",
        log,
        startup_timeout=timeout,
    )
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    assert not log.exists()


def test_borrow_close_preserves_the_running_service(
    executable: Path, tmp_path: Path
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        running = Running(
            identity=accepted(query(process.diagnostics)).identity,
            endpoint=process.diagnostics,
        )
        lease = accepted(Lease.borrow(running))
        assert lease.running == running
        assert accepted(lease.close()) is None and accepted(lease.close()) is None
        assert process.process.poll() is None


def test_borrow_disappeared_process_returns_error(
    executable: Path, tmp_path: Path
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        running = Running(
            identity=accepted(query(process.diagnostics)).identity,
            endpoint=process.diagnostics,
        )
    assert isinstance(Lease.borrow(running), Rejected)


def test_start_missing_executable_and_failed_broker_return_errors(
    executable: Path, tmp_path: Path
) -> None:
    assert isinstance(
        Lease.start(
            tmp_path / "missing",
            Path("/dev/null"),
            tmp_path / "s",
            tmp_path / "missing.log",
        ),
        Rejected,
    )
    assert isinstance(
        Lease.start(
            executable, Path("/dev/null"), tmp_path / "s", tmp_path / "failed.log"
        ),
        Rejected,
    )
    assert not (tmp_path / "s").exists()
