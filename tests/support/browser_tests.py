"""module: browser_tests; responsibility: define owned browser startup failures.

operations: pytest cases; entities: none; tests: timeout, cleanup, workspace safety.
Real playback behavior is covered by tests/browser through this public interface.
"""

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


def test_open_existing_workspace_does_not_replace_user_files(tmp_path: Path) -> None:
    directory = tmp_path / "browser"
    directory.mkdir()
    _ = (directory / "owned.txt").write_text("preserve")
    assert isinstance(Browser.open(directory, (), ()), Rejected)
    assert (directory / "owned.txt").read_text() == "preserve"
