"""module: performance_tests; responsibility: define command-line error contracts.

operations: pytest cases; entities: none; tests: help, argument errors.
The three measurement scenarios are exercised directly as tool commands.
"""

import subprocess
from pathlib import Path


def test_cli_help_and_invalid_options_do_not_require_hardware() -> None:
    command = ["uv", "run", "python", "-m", "tests.tools.performance"]
    help = subprocess.run(
        [*command, "--help"], capture_output=True, text=True, check=False
    )
    assert (
        help.returncode == 0 and "--codec" in help.stdout and "--output" in help.stdout
    )
    invalid = subprocess.run(
        [*command, "--unknown", "x"], capture_output=True, text=True, check=False
    )
    assert invalid.returncode == 2 and "unknown" in invalid.stderr


def test_cli_missing_video_fails_before_creating_output(tmp_path: Path) -> None:
    result = subprocess.run(
        [
            "uv",
            "run",
            "python",
            "-m",
            "tests.tools.performance",
            "--video",
            str(tmp_path / "missing"),
            "--output",
            str(tmp_path / "output"),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 2 and not (tmp_path / "output").exists()
