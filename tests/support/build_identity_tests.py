"""module: build_identity_tests; responsibility: define current build identities.

operations: pytest cases; entities: none; tests: compiler/note/digest contracts.
Only compiler process boundaries and files are substituted.
"""

import errno
import subprocess
from pathlib import Path

import pytest

from tests.support.build_identity import (
    service_binary,
    service_id,
)
from tests.support.outcome import Ok, Rejected


@pytest.fixture
def source_tree(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    root = tmp_path / "source"
    (root / "src").mkdir(parents=True)
    _ = (root / "src/main.rs").write_bytes(b"current service source")

    def compile_process(
        arguments: list[str], *, cwd: Path, capture_output: bool
    ) -> subprocess.CompletedProcess[bytes]:
        assert cwd == root and capture_output
        assert arguments == ["cargo", "build", "--offline"]
        target = root / "target/debug/v4l2-stateful-rkmpp"
        target.parent.mkdir(parents=True, exist_ok=True)
        _ = target.write_bytes((root / "src/main.rs").read_bytes())
        output = b""
        return subprocess.CompletedProcess(arguments, 0, output, b"")

    monkeypatch.setattr(subprocess, "run", compile_process)
    return root


@pytest.mark.parametrize("tool", ["objcopy", "cargo"])
def test_build_failed_tool_reports_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, tool: str
) -> None:
    def fail(
        arguments: list[str], *, cwd: Path, capture_output: bool
    ) -> subprocess.CompletedProcess[bytes]:
        assert cwd == tmp_path and capture_output
        return subprocess.CompletedProcess(
            arguments, 2 if arguments[0] == tool else 0, b"", b"compiler diagnostic"
        )

    monkeypatch.setattr(subprocess, "run", fail)
    result = (
        service_binary(tmp_path) if tool == "cargo" else service_id(tmp_path / "binary")
    )
    assert isinstance(result, Rejected)
    assert tool in result.operation and "compiler diagnostic" in result.operation


@pytest.mark.parametrize("build_service", [False, True])
def test_build_missing_compiler_is_rejected(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, build_service: bool
) -> None:
    def missing(
        arguments: list[str], *, cwd: Path, capture_output: bool
    ) -> subprocess.CompletedProcess[bytes]:
        assert arguments and cwd == tmp_path and capture_output
        raise FileNotFoundError(errno.ENOENT, "missing compiler")

    monkeypatch.setattr(subprocess, "run", missing)
    result = (
        service_binary(tmp_path) if build_service else service_id(tmp_path / "binary")
    )
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_service_binary_rebuilds_changed_source(source_tree: Path) -> None:
    first = service_binary(source_tree)
    assert (
        isinstance(first, Ok) and first.value.read_bytes() == b"current service source"
    )
    _ = (source_tree / "src/main.rs").write_bytes(b"edited service source")
    second = service_binary(source_tree)
    assert (
        isinstance(second, Ok) and second.value.read_bytes() == b"edited service source"
    )


def test_service_binary_missing_compiler_output_is_rejected(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    def no_output(
        arguments: list[str], *, cwd: Path, capture_output: bool
    ) -> subprocess.CompletedProcess[bytes]:
        assert arguments[0] == "cargo" and cwd == tmp_path and capture_output
        return subprocess.CompletedProcess(arguments, 0, b"", b"")

    monkeypatch.setattr(subprocess, "run", no_output)
    result = service_binary(tmp_path)
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_service_id_reads_gnu_build_note(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    def extract(
        arguments: list[str], *, cwd: Path, capture_output: bool
    ) -> subprocess.CompletedProcess[bytes]:
        assert arguments[0] == "objcopy" and capture_output and cwd == tmp_path
        note = (
            (4).to_bytes(4, "little")
            + (12).to_bytes(4, "little")
            + (3).to_bytes(4, "little")
            + b"GNU\0"
            + bytes(range(12))
        )
        return subprocess.CompletedProcess(arguments, 0, note, b"")

    monkeypatch.setattr(subprocess, "run", extract)
    result = service_id(tmp_path / "binary")
    assert isinstance(result, Ok) and result.value == bytes(range(12)).hex()


@pytest.mark.parametrize(
    "note",
    [
        b"",
        b"GNU",
        bytes(16),
        (4).to_bytes(4, "little")
        + (20).to_bytes(4, "little")
        + (3).to_bytes(4, "little")
        + b"GNU\0",
    ],
    ids=["empty-note", "truncated-header", "invalid-note-header", "missing-descriptor"],
)
def test_service_id_missing_or_malformed_note_is_rejected(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, note: bytes
) -> None:
    def extract(
        arguments: list[str], *, cwd: Path, capture_output: bool
    ) -> subprocess.CompletedProcess[bytes]:
        assert arguments[0] == "objcopy" and capture_output and cwd == tmp_path
        return subprocess.CompletedProcess(arguments, 0, note, b"")

    monkeypatch.setattr(subprocess, "run", extract)
    assert isinstance(service_id(tmp_path / "binary"), Rejected)
