"""module: tests.service.test_cli; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_cli_help_lists_production_commands
    test_cli_invalid_arguments_fail
    test_cli_missing_control_device_fails

CLI behavior belongs to process acceptance, not Rust module tests.
"""

import os
import subprocess
from pathlib import Path
from typing import Literal

import pytest


@pytest.mark.parametrize("arguments", [(), ("--help",)], ids=["default", "help"])
def test_cli_help_lists_production_commands(
    executable: Path, arguments: tuple[str, ...]
) -> None:
    result = subprocess.run(
        [str(executable), *arguments],
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )
    assert result.returncode == 0 and not result.stderr
    assert result.stdout == (
        "v4l2-stateful-rkmpp: V4L2 / MPP bridge service\nCommands: serve <control-device> <diagnostics-socket>\n"
    )


@pytest.mark.parametrize(
    "arguments", [("unknown",), ("--help", "extra"), ("serve", "a", "b", "extra")]
)
def test_cli_invalid_arguments_fail(
    executable: Path, arguments: tuple[str, ...]
) -> None:
    result = subprocess.run(
        [str(executable), *arguments],
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )
    assert result.returncode == 1 and "unknown command" in result.stderr


def test_cli_missing_control_device_fails(executable: Path, tmp_path: Path) -> None:
    result = subprocess.run(
        [
            str(executable),
            "serve",
            str(tmp_path / "missing"),
            str(tmp_path / "status.sock"),
        ],
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )
    assert result.returncode == 1 and "control" in result.stderr


@pytest.mark.parametrize("serve", [False, True])
def test_cli_non_utf8_arguments_report_errors_without_panicking(
    executable: Path, tmp_path: Path, serve: bool
) -> None:
    arguments = (
        [
            b"serve",
            os.fsencode(tmp_path) + b"/missing-\xff",
            os.fsencode(tmp_path / "status"),
        ]
        if serve
        else [b"\xff"]
    )
    result = subprocess.run(
        [os.fsencode(executable), *arguments],
        capture_output=True,
        timeout=10,
        check=False,
    )
    assert result.returncode == 1
    assert (b"control" if serve else b"unknown command") in result.stderr
    assert b"panicked" not in result.stderr


def test_cli_diagnostic_startup_failure_preserves_occupied_path(
    executable: Path, tmp_path: Path
) -> None:
    endpoint = tmp_path / "status"
    _ = endpoint.write_bytes(b"preserve")
    # Admission requires a pollable control endpoint. A FIFO keeps this test
    # focused on diagnostics startup without involving a real kernel device.
    control = tmp_path / "control"
    os.mkfifo(control, mode=0o600)
    result = subprocess.run(
        [str(executable), "serve", str(control), str(endpoint)],
        capture_output=True,
        text=True,
        timeout=5,
        check=False,
    )
    assert result.returncode == 1 and "open diagnostics" in result.stderr
    assert endpoint.read_bytes() == b"preserve"


@pytest.mark.parametrize("phase", ["control", "hardware"])
def test_executor_creation_failure_cleans_its_startup_stage(
    executable: Path, tmp_path: Path, phase: Literal["control", "hardware"]
) -> None:
    control = tmp_path / "control"
    os.mkfifo(control, mode=0o600)
    endpoint = tmp_path / "status"
    assert not endpoint.exists()
    # The diagnostics socket is a public startup milestone. Using it as the
    # marker admits transport threads and rejects the later hardware pool.
    marker = endpoint if phase == "hardware" else tmp_path / "thread-failure"
    if phase == "control":
        marker.touch()
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    assert library.is_file()
    environment = dict(os.environ)
    environment.update(
        LD_PRELOAD=str(library.resolve()),
        RKMPP_TEST_THREAD_FAILURE=str(marker),
    )
    result = subprocess.run(
        [str(executable), "serve", str(control), str(endpoint)],
        env=environment,
        capture_output=True,
        text=True,
        timeout=5,
        check=False,
    )
    assert result.returncode == 1
    assert "spawn worker thread" in result.stderr
    context = "open control " if phase == "control" else "spawn worker thread:"
    assert result.stderr.startswith(context)
    assert not endpoint.exists()
