"""module: telemetry_sampler_tests; responsibility: define workload sampling errors.

operations: pytest cases; entities: none; tests: accounting, lifetime, hardware data.
Only public counter providers and published file reads are substituted.
"""

import errno
import time
from pathlib import Path

import pytest

from tests.support import service_runtime
from tests.support.diagnostics import Control, Identity, Snapshot
from tests.support.outcome import Ok, Rejected, Result, accepted
from tests.support.service_runtime import Running
from tests.tools import cpu
from tests.tools.cpu import Process, Processes
from tests.tools.telemetry import Sampler


@pytest.fixture
def counters(
    monkeypatch: pytest.MonkeyPatch,
) -> tuple[Running, list[float], list[float]]:
    identity = Identity(
        pid=3,
        uid=1000,
        start_ticks=1,
        build_id="0123456789abcdef",
        control=Control(device=1, inode=1, raw_device=1),
    )
    running = Running(identity=identity, endpoint=Path("/status"))
    clock = [10.0]
    service_cpu = [1.0]

    def now() -> float:
        return clock[0]

    def processes(browser_pid: int, desktop_pid: int) -> Result[Processes]:
        assert (browser_pid, desktop_pid) == (1, 2)
        delta = int((clock[0] - 10) * 100)
        return Ok(
            {
                1: Process(
                    parent=0, start=1, ticks=delta, name="chromium", kind="browser"
                ),
                2: Process(
                    parent=0, start=1, ticks=0, name="gnome-shell", kind="browser"
                ),
            }
        )

    def sample(owner: Running) -> Result[Snapshot]:
        assert owner == running
        return Ok(
            Snapshot(
                protocol=1,
                identity=identity,
                state="ready",
                cpu_seconds=service_cpu[0],
                fds=2,
                threads=1,
                sessions=(),
            )
        )

    def read(path: Path, encoding: str | None = None, errors: str | None = None) -> str:
        assert encoding is None and errors is None
        return (
            "25@1000000000Hz"
            if "devfreq" in str(path)
            else "core0 load: 12.00% utilization: 15.00%"
        )

    monkeypatch.setattr(time, "monotonic", now)
    monkeypatch.setattr(cpu, "processes", processes)
    monkeypatch.setattr(service_runtime, "sample", sample)
    monkeypatch.setattr(Path, "read_text", read)
    return running, clock, service_cpu


def test_read_combines_owned_tree_and_service_self_cpu(
    counters: tuple[Running, list[float], list[float]],
) -> None:
    running, clock, service_cpu = counters
    sampler = accepted(Sampler.open(1, running, 2))
    clock[0] = 11
    service_cpu[0] = 1.25
    sample = accepted(sampler.read())
    assert sample.elapsed == 1 and sample.cpu["bridge"] == 25
    assert sample.cpu["chromium"] > 0 and sample.gpu_percent == 25
    assert sample.mpp == {"core0": 12}


def test_read_decreased_cpu_and_unavailable_status_are_errors(
    counters: tuple[Running, list[float], list[float]], monkeypatch: pytest.MonkeyPatch
) -> None:
    running, clock, service_cpu = counters
    sampler = accepted(Sampler.open(1, running, 2))
    clock[0] = 11
    service_cpu[0] = 0
    assert isinstance(sampler.read(), Rejected)

    def gone(owner: Running) -> Result[Snapshot]:
        assert owner == running
        return Rejected(errno.ESRCH, "service exited")

    monkeypatch.setattr(service_runtime, "sample", gone)
    assert isinstance(sampler.read(), Rejected)


def test_open_invalid_workload_pid_is_an_error(
    counters: tuple[Running, list[float], list[float]],
) -> None:
    running, _, _ = counters
    assert isinstance(Sampler.open(0, running, 2), Rejected)
