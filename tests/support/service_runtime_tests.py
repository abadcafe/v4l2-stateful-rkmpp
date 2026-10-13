"""module: service_runtime_tests; responsibility: define broker readiness decisions.

operations: pytest cases; entities: none; tests: locate, sample, identity stability.
Only public status and device-open boundaries are substituted.
"""

import errno
import os
from pathlib import Path

import pytest

from tests.support import diagnostics, service_runtime, syscalls
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
) -> tuple[Path, Path, Snapshot]:
    control, endpoint = (
        tmp_path / "control",
        tmp_path / "status.sock",
    )
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

    def busy(path: Path) -> Result[syscalls.OwnedFd]:
        assert path.is_file()
        return Rejected(errno.EBUSY, "broker occupied")

    def status(path: Path) -> Result[Snapshot]:
        assert path == endpoint
        return Ok(snapshot)

    monkeypatch.setattr(diagnostics, "query", status)
    monkeypatch.setattr(syscalls, "open_fd", busy)
    return control, endpoint, snapshot


def test_locate_matching_owner_returns_authenticated_identity(
    runtime: tuple[Path, Path, Snapshot],
) -> None:
    control, endpoint, snapshot = runtime
    owner = accepted(service_runtime.locate(endpoint, control))
    assert owner is not None and owner.identity == snapshot.identity
    assert accepted(service_runtime.sample(owner)) == snapshot


@pytest.mark.parametrize("code", [errno.ENOENT, errno.ECONNREFUSED])
def test_locate_absent_diagnostics_and_busy_broker_never_assumes_no_service(
    runtime: tuple[Path, Path, Snapshot],
    monkeypatch: pytest.MonkeyPatch,
    code: int,
) -> None:
    control, endpoint, _ = runtime
    _query(monkeypatch, Rejected(code, "no endpoint"))
    result = service_runtime.locate(endpoint, control)
    assert isinstance(result, Rejected) and result.code == errno.EBUSY
    assert "Manually stop" in result.operation and "diagnostics" in result.operation


@pytest.mark.parametrize("code", [errno.ENOENT, errno.ECONNREFUSED])
def test_locate_absent_diagnostics_and_free_broker_allows_latest_start(
    runtime: tuple[Path, Path, Snapshot],
    monkeypatch: pytest.MonkeyPatch,
    code: int,
) -> None:
    control, endpoint, _ = runtime
    _query(monkeypatch, Rejected(code, "no endpoint"))
    monkeypatch.setattr(syscalls, "open_fd", _free)
    assert accepted(service_runtime.locate(endpoint, control)) is None


@pytest.mark.parametrize("code", [errno.EACCES, errno.EPROTO, errno.ETIMEDOUT])
def test_locate_unreadable_or_invalid_status_is_an_error(
    runtime: tuple[Path, Path, Snapshot],
    monkeypatch: pytest.MonkeyPatch,
    code: int,
) -> None:
    control, endpoint, _ = runtime
    _query(monkeypatch, Rejected(code, "invalid status"))
    result = service_runtime.locate(endpoint, control)
    assert isinstance(result, Rejected) and result.code == code


def test_locate_wrong_control_identity_is_rejected(
    runtime: tuple[Path, Path, Snapshot], tmp_path: Path
) -> None:
    _, endpoint, _ = runtime
    other = tmp_path / "other-control"
    _ = other.write_bytes(b"")
    result = service_runtime.locate(endpoint, other)
    assert isinstance(result, Rejected) and result.code == errno.ESTALE


def test_locate_status_without_broker_ownership_is_rejected(
    runtime: tuple[Path, Path, Snapshot], monkeypatch: pytest.MonkeyPatch
) -> None:
    control, endpoint, _ = runtime
    monkeypatch.setattr(syscalls, "open_fd", _free)
    result = service_runtime.locate(endpoint, control)
    assert isinstance(result, Rejected) and result.code == errno.EPROTO


@pytest.mark.parametrize("field", ["pid", "start_ticks", "build_id"])
def test_sample_replaced_service_is_rejected(
    runtime: tuple[Path, Path, Snapshot],
    monkeypatch: pytest.MonkeyPatch,
    field: str,
) -> None:
    control, endpoint, snapshot = runtime
    owner = accepted(service_runtime.locate(endpoint, control))
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
    runtime: tuple[Path, Path, Snapshot], monkeypatch: pytest.MonkeyPatch
) -> None:
    control, endpoint, snapshot = runtime
    owner = accepted(service_runtime.locate(endpoint, control))
    assert owner is not None
    _query(monkeypatch, Ok(snapshot.model_copy(update={"state": "stopping"})))
    result = service_runtime.sample(owner)
    assert isinstance(result, Rejected) and result.code == errno.ESHUTDOWN


def test_locate_stopping_service_is_rejected(
    runtime: tuple[Path, Path, Snapshot], monkeypatch: pytest.MonkeyPatch
) -> None:
    control, endpoint, snapshot = runtime
    _query(monkeypatch, Ok(snapshot.model_copy(update={"state": "stopping"})))
    result = service_runtime.locate(endpoint, control)
    assert isinstance(result, Rejected) and result.code == errno.ESHUTDOWN


@pytest.mark.parametrize("code", [errno.EACCES, errno.ENOENT])
def test_locate_inaccessible_control_preserves_error(
    runtime: tuple[Path, Path, Snapshot], monkeypatch: pytest.MonkeyPatch, code: int
) -> None:
    control, endpoint, _ = runtime

    def inaccessible(path: Path) -> Result[syscalls.OwnedFd]:
        assert path == control
        return Rejected(code, "control unavailable")

    monkeypatch.setattr(syscalls, "open_fd", inaccessible)
    result = service_runtime.locate(endpoint, control)
    assert isinstance(result, Rejected) and result.code == code


def test_require_idle_running_service_requests_manual_stop(
    runtime: tuple[Path, Path, Snapshot],
) -> None:
    control, endpoint, snapshot = runtime
    result = service_runtime.require_idle(endpoint, control)
    assert isinstance(result, Rejected) and result.code == errno.EBUSY
    assert str(snapshot.identity.pid) in result.operation
    assert "Manually stop" in result.operation


@pytest.mark.parametrize("code", [errno.ENOENT, errno.ECONNREFUSED])
def test_require_idle_absent_service_and_free_control_allows_start(
    runtime: tuple[Path, Path, Snapshot], monkeypatch: pytest.MonkeyPatch, code: int
) -> None:
    control, endpoint, _ = runtime
    _query(monkeypatch, Rejected(code, "no endpoint"))
    monkeypatch.setattr(syscalls, "open_fd", _free)
    assert accepted(service_runtime.require_idle(endpoint, control)) is None


@pytest.mark.parametrize("code", [errno.EPROTO, errno.EACCES])
def test_require_idle_unverifiable_service_preserves_error(
    runtime: tuple[Path, Path, Snapshot], monkeypatch: pytest.MonkeyPatch, code: int
) -> None:
    control, endpoint, _ = runtime
    _query(monkeypatch, Rejected(code, "invalid diagnostics"))
    result = service_runtime.require_idle(endpoint, control)
    assert isinstance(result, Rejected) and result.code == code
