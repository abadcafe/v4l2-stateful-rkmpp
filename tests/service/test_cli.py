"""module: tests.service.test_cli; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_cli_help_lists_production_commands
    test_cli_invalid_arguments_fail
    test_cli_missing_control_device_fails

CLI behavior belongs to process acceptance, not Rust module tests.
"""

import subprocess
from pathlib import Path

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


def test_cli_diagnostic_startup_failure_preserves_occupied_path(
    executable: Path, tmp_path: Path
) -> None:
    endpoint = tmp_path / "status"
    _ = endpoint.write_bytes(b"preserve")
    result = subprocess.run(
        [str(executable), "serve", "/dev/null", str(endpoint)],
        capture_output=True,
        text=True,
        timeout=5,
        check=False,
    )
    assert result.returncode == 1 and "open diagnostics" in result.stderr
    assert endpoint.read_bytes() == b"preserve"
