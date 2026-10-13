"""module: kernel_runtime_tests; responsibility: define real-kernel readiness.

operations: pytest cases; entities: none; tests: loaded identity/exclusive access.
Only compiler process boundaries, sysfs files, and open syscall are substituted.
"""

import errno
import os
import subprocess
from pathlib import Path

import pytest

from tests.support.kernel_runtime import exclusive, verify
from tests.support.outcome import Ok, Rejected


@pytest.fixture
def kernel_tree(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[Path, Path]:
    root, modules = tmp_path / "source", tmp_path / "sys/module"
    (root / "kernel").mkdir(parents=True)
    modules.mkdir(parents=True)
    _ = (root / "kernel/device.c").write_bytes(b"new module note")

    def compile_process(
        arguments: list[str], *, cwd: Path, capture_output: bool
    ) -> subprocess.CompletedProcess[bytes]:
        assert cwd == root and capture_output
        output = (
            (root / "kernel/device.c").read_bytes()
            if arguments[0] == "objcopy"
            else b""
        )
        return subprocess.CompletedProcess(arguments, 0, output, b"")

    monkeypatch.setattr(subprocess, "run", compile_process)
    return root, modules


def _loaded(modules: Path, note: bytes) -> None:
    notes = modules / "v4l2_rkmpp_bridge/notes"
    notes.mkdir(parents=True)
    _ = (notes / ".note.gnu.build-id").write_bytes(note)


def test_verify_matching_loaded_build_is_accepted(
    kernel_tree: tuple[Path, Path],
) -> None:
    root, modules = kernel_tree
    _loaded(modules, b"new module note")
    result = verify(root, modules)
    assert isinstance(result, Ok) and result.value == Path("/dev/v4l2-rkmpp-control")


def test_verify_changed_source_requires_manual_module_update(
    kernel_tree: tuple[Path, Path],
) -> None:
    root, modules = kernel_tree
    _loaded(modules, b"new module note")
    _ = (root / "kernel/device.c").write_bytes(b"edited module note")
    result = verify(root, modules)
    assert isinstance(result, Rejected) and result.code == errno.ESTALE
    assert "Manually" in result.operation and "install" in result.operation
    assert "v4l2_rkmpp_bridge" in result.operation and "stop" in result.operation


def test_verify_unloaded_module_requires_manual_install(
    kernel_tree: tuple[Path, Path],
) -> None:
    root, modules = kernel_tree
    result = verify(root, modules)
    assert isinstance(result, Rejected) and result.code == errno.ENOENT
    assert "not loaded" in result.operation and "Manually" in result.operation


def test_verify_loaded_module_without_identity_is_rejected(
    kernel_tree: tuple[Path, Path],
) -> None:
    root, modules = kernel_tree
    (modules / "v4l2_rkmpp_bridge").mkdir()
    result = verify(root, modules)
    assert isinstance(result, Rejected)
    assert "build ID" in result.operation and "install" in result.operation


def test_verify_failed_rebuild_does_not_accept_old_module(
    kernel_tree: tuple[Path, Path], monkeypatch: pytest.MonkeyPatch
) -> None:
    root, modules = kernel_tree
    _loaded(modules, b"new module note")

    def fail(
        arguments: list[str], *, cwd: Path, capture_output: bool
    ) -> subprocess.CompletedProcess[bytes]:
        assert arguments[0] == "make" and cwd == root and capture_output
        return subprocess.CompletedProcess(arguments, 2, b"", b"invalid C source")

    monkeypatch.setattr(subprocess, "run", fail)
    result = verify(root, modules)
    assert isinstance(result, Rejected) and "invalid C source" in result.operation


def test_exclusive_available_control_releases_descriptor(tmp_path: Path) -> None:
    control = tmp_path / "control"
    _ = control.write_bytes(b"")
    before = len(tuple(Path("/proc/self/fd").iterdir()))
    result = exclusive(control)
    assert isinstance(result, Ok) and result.value is None
    assert len(tuple(Path("/proc/self/fd").iterdir())) == before


@pytest.mark.parametrize("code", [errno.EBUSY, errno.EACCES, errno.EPERM])
def test_exclusive_unavailable_control_is_rejected(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, code: int
) -> None:
    def unavailable(path: Path, flags: int) -> int:
        assert path == tmp_path / "control" and flags & os.O_RDWR
        raise OSError(code, "unavailable control")

    monkeypatch.setattr(os, "open", unavailable)
    result = exclusive(tmp_path / "control")
    assert isinstance(result, Rejected) and result.code == code
    if code == errno.EBUSY:
        assert (
            "Manually stop" in result.operation
            and "Kernel integration tests" in result.operation
        )
    else:
        assert "video" in result.operation and "0660" in result.operation
        assert "udev" in result.operation and "Root is not required" in result.operation


def test_exclusive_missing_control_is_rejected(tmp_path: Path) -> None:
    result = exclusive(tmp_path / "missing")
    assert isinstance(result, Rejected) and result.code == errno.ENOENT
