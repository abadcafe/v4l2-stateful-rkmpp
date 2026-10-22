"""module: browser_tests; responsibility: define owned browser startup failures.

operations: pytest cases; entities: none; tests: timeout, cleanup, workspace safety.
Real playback behavior is covered by tests/browser through this public interface.
"""

import errno
import os
import signal
import subprocess
from pathlib import Path
from typing import BinaryIO

import pytest

from tests.support import browser_launch
from tests.support.browser import Browser
from tests.support.cdp import Cdp
from tests.support.outcome import Ok, Rejected, Result, accepted


def test_open_waits_for_execution_context_after_navigation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    evaluate = Cdp.evaluate
    interrupted = False

    def during_navigation[T](
        channel: Cdp, expression: str, model: type[T]
    ) -> Result[T]:
        nonlocal interrupted
        if expression == "document.readyState" and not interrupted:
            interrupted = True
            return Rejected(-32000, "Inspected target navigated or closed")
        return evaluate(channel, expression, model)

    monkeypatch.setattr(Cdp, "evaluate", during_navigation)
    browser = accepted(Browser.open(tmp_path / "browser", (), ()))
    try:
        assert interrupted and browser.alive
        assert accepted(browser.tab.evaluate("document.readyState", str)) == "complete"
    finally:
        accepted(browser.close())


def test_open_missing_browser_returns_error_without_leaking_server(
    tmp_path: Path,
) -> None:
    result = Browser.open(tmp_path / "browser", (), (), executable=tmp_path / "missing")
    assert isinstance(result, Rejected)


def test_open_timeout_terminates_only_owned_process_group(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    processes: list[subprocess.Popen[bytes]] = []

    def launch(
        profile: Path, log: BinaryIO, flags: tuple[str, ...], executable: Path
    ) -> Result[subprocess.Popen[bytes]]:
        assert (
            profile.parent == tmp_path / "browser"
            and not flags
            and executable.is_file()
        )
        process = subprocess.Popen(
            ["/usr/bin/sleep", "30"], stdout=log, stderr=log, start_new_session=True
        )
        processes.append(process)
        return Ok(process)

    monkeypatch.setattr(browser_launch, "launch", launch)
    result = Browser.open(tmp_path / "browser", (), (), startup_timeout=0.05)
    assert isinstance(result, Rejected)
    assert len(processes) == 1 and processes[0].poll() is not None


def test_open_exited_leader_retains_group_identity_until_cleanup_signals(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    processes: list[subprocess.Popen[bytes]] = []
    attempts: list[tuple[int, int, bool]] = []

    def launch(
        profile: Path, log: BinaryIO, flags: tuple[str, ...], executable: Path
    ) -> Result[subprocess.Popen[bytes]]:
        assert (
            profile.parent == tmp_path / "browser"
            and not flags
            and executable.is_file()
        )
        process: subprocess.Popen[bytes] = subprocess.Popen(
            ["/usr/bin/true"], stdout=log, stderr=log, start_new_session=True
        )
        # Observe exit while preserving the unreaped child and its identifier.
        status = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOWAIT)
        assert status is not None and status.si_code == os.CLD_EXITED
        processes.append(process)
        return Ok(process)

    def stop_group(group: int, number: int) -> None:
        process = processes[0]
        pinned = False
        if process.returncode is None:
            status = os.waitid(
                os.P_PID, process.pid, os.WEXITED | os.WNOWAIT | os.WNOHANG
            )
            pinned = status is not None and status.si_pid == process.pid
        attempts.append((group, number, pinned))
        # Record the boundary without sending any real process-group signal.
        raise ProcessLookupError(errno.ESRCH, "owned group already exited")

    monkeypatch.setattr(browser_launch, "launch", launch)
    monkeypatch.setattr(os, "killpg", stop_group)
    result = Browser.open(tmp_path / "browser", (), (), startup_timeout=0.1)
    assert isinstance(result, Rejected) and result.code == errno.ETIMEDOUT
    assert len(processes) == 1 and processes[0].wait(timeout=1) == 0
    assert attempts == [(processes[0].pid, signal.SIGTERM, True)]


def test_open_existing_workspace_does_not_replace_user_files(tmp_path: Path) -> None:
    directory = tmp_path / "browser"
    directory.mkdir()
    _ = (directory / "owned.txt").write_text("preserve")
    assert isinstance(Browser.open(directory, (), ()), Rejected)
    assert (directory / "owned.txt").read_text() == "preserve"


@pytest.mark.parametrize(
    "timeout", [0.0, -1.0, float("nan"), float("inf"), -float("inf")]
)
def test_open_invalid_deadline_rejects_before_allocating_workspace(
    tmp_path: Path, timeout: float
) -> None:
    directory = tmp_path / "browser"
    result = Browser.open(
        directory, (), (), executable=tmp_path / "missing", startup_timeout=timeout
    )
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    assert not directory.exists()
