"""module: kernel_runtime_tests; responsibility: define real-kernel readiness.

operations: pytest cases; entities: none; tests: loaded module/exclusive access.
Only sysfs files and the open syscall are substituted.
"""

import errno
import os
from pathlib import Path

import pytest

from tests.support.kernel_runtime import exclusive, verify
from tests.support.outcome import Ok, Rejected


def test_verify_loaded_module_is_ready(tmp_path: Path) -> None:
    (tmp_path / "v4l2_rkmpp_bridge").mkdir()
    result = verify(tmp_path)
    assert isinstance(result, Ok) and result.value == Path("/dev/v4l2-rkmpp-control")


def test_verify_unloaded_module_requires_manual_load(tmp_path: Path) -> None:
    result = verify(tmp_path)
    assert isinstance(result, Rejected) and result.code == errno.ENOENT
    assert "not loaded" in result.operation and "Manually" in result.operation


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
