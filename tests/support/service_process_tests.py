"""module: service_process_tests; responsibility: define daemon lifetime ownership.

operations: pytest cases; entities: none; tests: borrowed ownership and startup failure.
Real successful starts are exercised by the browser acceptance fixture.
"""

import errno
from pathlib import Path

import pytest

from tests.support.diagnostics import query
from tests.support.mock_process import Process
from tests.support.outcome import Rejected, accepted
from tests.support.service_process import Lease
from tests.support.service_runtime import Running


@pytest.mark.parametrize(
    "timeout", [0.0, -1.0, float("nan"), float("inf"), -float("inf")]
)
def test_start_invalid_deadline_rejects_before_opening_log(
    tmp_path: Path, timeout: float
) -> None:
    log = tmp_path / "service.log"
    result = Lease.start(
        tmp_path / "missing",
        Path("/dev/null"),
        tmp_path / "status",
        log,
        startup_timeout=timeout,
    )
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    assert not log.exists()


def test_borrow_close_preserves_the_running_service(
    executable: Path, tmp_path: Path
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        running = Running(
            identity=accepted(query(process.diagnostics)).identity,
            endpoint=process.diagnostics,
        )
        lease = accepted(Lease.borrow(running))
        assert lease.running == running
        assert accepted(lease.close()) is None and accepted(lease.close()) is None
        assert process.process.poll() is None


def test_borrow_disappeared_process_returns_error(
    executable: Path, tmp_path: Path
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        running = Running(
            identity=accepted(query(process.diagnostics)).identity,
            endpoint=process.diagnostics,
        )
    assert isinstance(Lease.borrow(running), Rejected)


def test_start_missing_executable_and_failed_broker_return_errors(
    executable: Path, tmp_path: Path
) -> None:
    assert isinstance(
        Lease.start(
            tmp_path / "missing",
            Path("/dev/null"),
            tmp_path / "s",
            tmp_path / "missing.log",
        ),
        Rejected,
    )
    assert isinstance(
        Lease.start(
            executable, Path("/dev/null"), tmp_path / "s", tmp_path / "failed.log"
        ),
        Rejected,
    )
    assert not (tmp_path / "s").exists()
