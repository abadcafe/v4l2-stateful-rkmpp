"""module: identity_tests; responsibility: run the kernel identity contract.

operations: test_identity_admission_preserves_exhausted_values; entities: none;
tests: the sibling C module's public 32/64-bit admission boundary.
"""

import subprocess
from pathlib import Path


def test_identity_admission_preserves_exhausted_values(tmp_path: Path) -> None:
    root = Path(__file__).resolve().parents[2]
    executable = tmp_path / "identity-contract"
    built = subprocess.run(
        [
            "cc",
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            str(root / "kernel/session/identity_tests.c"),
            "-o",
            str(executable),
        ],
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    assert built.returncode == 0, built.stderr
    tested = subprocess.run(
        [str(executable)],
        capture_output=True,
        text=True,
        timeout=5,
        check=False,
    )
    assert tested.returncode == 0, tested.stderr
