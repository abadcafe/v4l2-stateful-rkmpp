"""module: browser_launch_tests; responsibility: define direct desktop launching.

operations: pytest cases; entities: none; tests: inherited environment, ownership.
Only the process-creation boundary is substituted; no desktop is discovered.
"""

import os
import subprocess
from pathlib import Path
from typing import BinaryIO

import pytest

from tests.support.browser_launch import launch
from tests.support.outcome import Rejected, accepted


def test_launch_inherits_current_session_and_requests_background_start(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    environment = {
        "WAYLAND_DISPLAY": "custom-wayland",
        "XDG_RUNTIME_DIR": "/custom/runtime",
        "DBUS_SESSION_BUS_ADDRESS": "unix:abstract=custom-bus",
        "DISPLAY": ":123",
        "CUSTOM_SESSION_VALUE": "unchanged",
    }
    for name, value in environment.items():
        monkeypatch.setenv(name, value)
    monkeypatch.setenv("XDG_ACTIVATION_TOKEN", "front")
    monkeypatch.setenv("DESKTOP_STARTUP_ID", "front")
    spawn = subprocess.Popen

    def start(
        arguments: list[str],
        *,
        env: dict[str, str],
        stdout: BinaryIO,
        stderr: BinaryIO,
        start_new_session: bool,
    ) -> subprocess.Popen[bytes]:
        assert arguments[0] == "/usr/lib/chromium/chromium"
        assert (
            "--no-startup-window" in arguments
            and "--ozone-platform=wayland" in arguments
        )
        assert all(env[name] == value for name, value in environment.items())
        assert "XDG_ACTIVATION_TOKEN" not in env and "DESKTOP_STARTUP_ID" not in env
        assert stdout is stderr and start_new_session
        return spawn(["/usr/bin/true"], stdout=stdout, stderr=stderr)

    monkeypatch.setattr(subprocess, "Popen", start)
    profile = tmp_path / "profile"
    with (tmp_path / "browser.log").open("wb") as log:
        process = accepted(launch(profile, log, ("--ozone-platform=wayland",)))
        assert process.wait(timeout=5) == 0
    assert profile.stat().st_uid == os.geteuid()
    assert profile.stat().st_mode & 0o777 == 0o700
    assert os.environ["XDG_ACTIVATION_TOKEN"] == "front"


def test_launch_missing_binary_is_a_result_error(tmp_path: Path) -> None:
    with (tmp_path / "browser.log").open("wb") as log:
        result = launch(tmp_path / "profile", log, (), executable=tmp_path / "missing")
    assert isinstance(result, Rejected)
