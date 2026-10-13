"""module: service_runtime_tests; responsibility: define broker freshness decisions.

operations: pytest cases; entities: none; tests: verify, sample, identity stability.
Only public status, compiler and device-open boundaries are substituted.
"""

import errno
import os
from pathlib import Path

import pytest

from tests.support import build_identity, diagnostics, service_runtime, syscalls
from tests.support.diagnostics import Control, Identity, Snapshot
from tests.support.outcome import Ok, Rejected, Result, accepted


def _query(monkeypatch: pytest.MonkeyPatch, response: Result[Snapshot]) -> None:
    def status(_: Path) -> Result[Snapshot]:
        return response

    monkeypatch.setattr(diagnostics, "query", status)


def _free(path: Path) -> Result[syscalls.OwnedFd]:
    return Ok(syscalls.OwnedFd(os.open(path, os.O_RDWR)))


@pytest.fixture
def runtime(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[Path, Path, Path, Snapshot]:
    binary, control, endpoint = (
        tmp_path / "binary",
        tmp_path / "control",
        tmp_path / "status.sock",
    )
    _ = binary.write_bytes(b"linked image")
    _ = control.write_bytes(b"")
    metadata = control.stat()
    snapshot = Snapshot(
        protocol=1,
        identity=Identity(
            pid=os.getpid(),
            uid=os.geteuid(),
            start_ticks=10,
            build_id="0123456789abcdef",
            control=Control(
                device=metadata.st_dev,
                inode=metadata.st_ino,
                raw_device=metadata.st_rdev,
            ),
        ),
        state="ready",
        cpu_seconds=1.0,
        fds=4,
        threads=2,
        sessions=(),
    )

    def identify(path: Path) -> Result[str]:
        return (
            Ok("0123456789abcdef")
            if path.is_file()
            else Rejected(errno.ENOENT, "missing build")
        )

    def busy(path: Path) -> Result[syscalls.OwnedFd]:
        assert path.is_file()
        return Rejected(errno.EBUSY, "broker occupied")

    def status(path: Path) -> Result[Snapshot]:
        assert path == endpoint
        return Ok(snapshot)

    monkeypatch.setattr(build_identity, "service_id", identify)
    monkeypatch.setattr(diagnostics, "query", status)
    monkeypatch.setattr(syscalls, "open_fd", busy)
    return binary, control, endpoint, snapshot


def test_verify_matching_owner_returns_authenticated_identity(
    runtime: tuple[Path, Path, Path, Snapshot],
) -> None:
    binary, control, endpoint, snapshot = runtime
    owner = accepted(service_runtime.verify((binary,), endpoint, control))
    assert owner is not None and owner.identity == snapshot.identity
    assert accepted(service_runtime.sample(owner)) == snapshot


def test_verify_old_service_requests_manual_stop_and_automatic_latest_start(
    runtime: tuple[Path, Path, Path, Snapshot], monkeypatch: pytest.MonkeyPatch
) -> None:
    binary, control, endpoint, snapshot = runtime
    old = snapshot.model_copy(
        update={
            "identity": snapshot.identity.model_copy(
                update={"build_id": "fedcba9876543210"}
            )
        }
    )
    _query(monkeypatch, Ok(old))
    result = service_runtime.verify((binary,), endpoint, control)
    assert isinstance(result, Rejected) and result.code == errno.ESTALE
    assert str(os.getpid()) in result.operation
    assert (
        "Manually stop" in result.operation
        and "No manual service start is required" in result.operation
    )


@pytest.mark.parametrize("code", [errno.ENOENT, errno.ECONNREFUSED])
def test_verify_absent_diagnostics_and_busy_broker_never_assumes_no_service(
    runtime: tuple[Path, Path, Path, Snapshot],
    monkeypatch: pytest.MonkeyPatch,
    code: int,
) -> None:
    binary, control, endpoint, _ = runtime
    _query(monkeypatch, Rejected(code, "no endpoint"))
    result = service_runtime.verify((binary,), endpoint, control)
    assert isinstance(result, Rejected) and result.code == errno.EBUSY
    assert "Manually stop" in result.operation and "diagnostics" in result.operation


@pytest.mark.parametrize("code", [errno.ENOENT, errno.ECONNREFUSED])
def test_verify_absent_diagnostics_and_free_broker_allows_latest_start(
    runtime: tuple[Path, Path, Path, Snapshot],
    monkeypatch: pytest.MonkeyPatch,
    code: int,
) -> None:
    binary, control, endpoint, _ = runtime
    _query(monkeypatch, Rejected(code, "no endpoint"))
    monkeypatch.setattr(syscalls, "open_fd", _free)
    assert accepted(service_runtime.verify((binary,), endpoint, control)) is None


@pytest.mark.parametrize("code", [errno.EACCES, errno.EPROTO, errno.ETIMEDOUT])
def test_verify_unreadable_or_invalid_status_is_an_error(
    runtime: tuple[Path, Path, Path, Snapshot],
    monkeypatch: pytest.MonkeyPatch,
    code: int,
) -> None:
    binary, control, endpoint, _ = runtime
    _query(monkeypatch, Rejected(code, "invalid status"))
    result = service_runtime.verify((binary,), endpoint, control)
    assert isinstance(result, Rejected) and result.code == code


def test_verify_empty_expected_builds_is_rejected(tmp_path: Path) -> None:
    result = service_runtime.verify((), tmp_path / "status", tmp_path / "control")
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_verify_missing_expected_build_is_rejected(
    runtime: tuple[Path, Path, Path, Snapshot],
) -> None:
    binary, control, endpoint, _ = runtime
    binary.unlink()
    result = service_runtime.verify((binary,), endpoint, control)
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_verify_wrong_control_identity_is_rejected(
    runtime: tuple[Path, Path, Path, Snapshot], tmp_path: Path
) -> None:
    binary, _, endpoint, _ = runtime
    other = tmp_path / "other-control"
    _ = other.write_bytes(b"")
    result = service_runtime.verify((binary,), endpoint, other)
    assert isinstance(result, Rejected) and result.code == errno.ESTALE


def test_verify_status_without_broker_ownership_is_rejected(
    runtime: tuple[Path, Path, Path, Snapshot], monkeypatch: pytest.MonkeyPatch
) -> None:
    binary, control, endpoint, _ = runtime
    monkeypatch.setattr(syscalls, "open_fd", _free)
    result = service_runtime.verify((binary,), endpoint, control)
    assert isinstance(result, Rejected) and result.code == errno.EPROTO


@pytest.mark.parametrize("field", ["pid", "start_ticks", "build_id"])
def test_sample_replaced_service_is_rejected(
    runtime: tuple[Path, Path, Path, Snapshot],
    monkeypatch: pytest.MonkeyPatch,
    field: str,
) -> None:
    binary, control, endpoint, snapshot = runtime
    owner = accepted(service_runtime.verify((binary,), endpoint, control))
    assert owner is not None
    replacement = snapshot.model_copy(
        update={
            "identity": snapshot.identity.model_copy(
                update={field: "fedcba9876543210" if field == "build_id" else 99999}
            )
        }
    )
    _query(monkeypatch, Ok(replacement))
    result = service_runtime.sample(owner)
    assert isinstance(result, Rejected) and result.code == errno.ESTALE


def test_sample_stopping_service_is_rejected(
    runtime: tuple[Path, Path, Path, Snapshot], monkeypatch: pytest.MonkeyPatch
) -> None:
    binary, control, endpoint, snapshot = runtime
    owner = accepted(service_runtime.verify((binary,), endpoint, control))
    assert owner is not None
    _query(monkeypatch, Ok(snapshot.model_copy(update={"state": "stopping"})))
    result = service_runtime.sample(owner)
    assert isinstance(result, Rejected) and result.code == errno.ESHUTDOWN
