"""module: build_identity; responsibility: identify builds of the current source.

operations: service_binary, existing_service_binary, service_id; entities: none;
tests: build_identity_tests. Builds stay in the repository; nothing is installed.
"""

import errno
import os
import subprocess
from pathlib import Path

from tests.support.outcome import Ok, Rejected, Result


def _run(root: Path, arguments: list[str]) -> Result[bytes]:
    try:
        process = subprocess.run(arguments, cwd=root, capture_output=True)
    except OSError as error:
        return Rejected(error.errno or 0, f"Execute {' '.join(arguments)}: {error}")
    if process.returncode != 0:
        output = (process.stdout + process.stderr).decode(errors="replace")
        return Rejected(errno.EIO, f"Command {' '.join(arguments)} failed:\n{output}")
    return Ok(process.stdout)


def existing_service_binary(root: Path) -> Result[Path]:
    binary = root / "target/debug/v4l2-stateful-rkmpp"
    if not binary.is_file():
        return Rejected(
            errno.ENOENT,
            f"Service binary has not been built: {binary}. Run cargo build in the project root before running browser integration tests.",
        )
    if not os.access(binary, os.X_OK):
        return Rejected(errno.EACCES, f"Service binary is not executable: {binary}")
    return Ok(binary)


def service_binary(root: Path) -> Result[Path]:
    return _service(
        root, ["cargo", "build", "--offline"], root / "target/debug/v4l2-stateful-rkmpp"
    )


def _service(root: Path, arguments: list[str], binary: Path) -> Result[Path]:
    built = _run(root, arguments)
    if isinstance(built, Rejected):
        return built
    if not binary.is_file():
        return Rejected(
            errno.ENOENT,
            f"Command {' '.join(arguments)} did not produce the service binary: {binary}",
        )
    return Ok(binary)


def service_id(path: Path) -> Result[str]:
    note = _run(
        path.parent,
        [
            "objcopy",
            "-O",
            "binary",
            "--only-section=.note.gnu.build-id",
            str(path.resolve()),
            "/dev/stdout",
        ],
    )
    if isinstance(note, Rejected):
        return note
    raw = note.value
    if len(raw) < 16:
        return Rejected(errno.ENODATA, f"Service binary {path} has no GNU build ID")
    name, length, kind = (
        int.from_bytes(raw[begin : begin + 4], "little") for begin in (0, 4, 8)
    )
    if (
        name != 4
        or kind != 3
        or raw[12:16] != b"GNU\0"
        or not 8 <= length <= 64
        or len(raw) != 16 + (length + 3) // 4 * 4
    ):
        return Rejected(
            errno.EPROTO, f"Service binary {path} has an invalid GNU build ID"
        )
    return Ok(raw[16 : 16 + length].hex())
