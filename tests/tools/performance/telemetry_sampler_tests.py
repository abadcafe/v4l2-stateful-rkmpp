"""module: telemetry_sampler_tests; responsibility: define workload sampling errors.

operations: pytest cases; entities: none; tests: accounting, lifetime, hardware data.
Only public counter providers and published file reads are substituted.
"""

import errno
import time
from pathlib import Path

import pytest

from tests.support import service_runtime
from tests.support.diagnostics import Control, Identity, Measurement
from tests.support.outcome import Ok, Rejected, Result, accepted
from tests.support.service_runtime import Running
from tests.tools.performance import cpu
from tests.tools.performance.cpu import Process, Processes
from tests.tools.performance.telemetry import Sampler


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

    def processes(browser_pid: int, desktop_pid: int | None) -> Result[Processes]:
        assert browser_pid == 1 and desktop_pid in (None, 2)
        delta = int((clock[0] - 10) * 100)
        return Ok(
            {
                1: Process(
                    parent=0, start=1, ticks=delta, name="chromium", kind="browser"
                ),
                2: Process(parent=0, start=1, ticks=0, name="desktop", kind="browser"),
            }
        )

    def sample(owner: Running) -> Result[Measurement]:
        assert owner == running
        return Ok(
            Measurement(
                protocol=1,
                identity=identity,
                state="ready",
                cpu_seconds=service_cpu[0],
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
    monkeypatch.setattr(service_runtime, "measure", sample)
    monkeypatch.setattr(Path, "read_text", read)
    return running, clock, service_cpu


def test_read_combines_owned_tree_and_service_self_cpu(
    counters: tuple[Running, list[float], list[float]],
) -> None:
    running, clock, service_cpu = counters
    sampler = accepted(Sampler.open(1, running, 2, origin=10))
    clock[0] = 11
    service_cpu[0] = 1.25
    sample = accepted(sampler.read())
    assert sample.elapsed == 1 and sample.cpu["bridge"].percent == 25
    rate = sample.cpu["chromium"].percent
    assert rate is not None and rate > 0 and sample.gpu_percent == 25
    assert sample.mpp_completed_window_load == {"core0": 12}


def test_read_uses_the_measurements_origin_before_counter_initialization(
    counters: tuple[Running, list[float], list[float]],
) -> None:
    running, clock, _ = counters
    sampler = accepted(Sampler.open(1, running, origin=5))
    clock[0] = 11
    sample = accepted(sampler.read())
    assert sample.elapsed == 6
    assert sample.cpu["chromium"].interval_start == 5
    assert sample.cpu["bridge"].interval_start == 5


@pytest.mark.parametrize("origin", [float("nan"), float("inf"), -1.0, 11.0])
def test_open_rejects_an_invalid_measurement_origin(
    counters: tuple[Running, list[float], list[float]], origin: float
) -> None:
    running, _, _ = counters
    result = Sampler.open(1, running, origin=origin)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_read_accounts_for_each_counter_acquisition_interval(
    counters: tuple[Running, list[float], list[float]], monkeypatch: pytest.MonkeyPatch
) -> None:
    running, clock, service_cpu = counters
    processes, measure = cpu.processes, service_runtime.measure
    delay = [2.0]

    def delayed_processes(
        browser_pid: int, desktop_pid: int | None
    ) -> Result[Processes]:
        clock[0] += delay[0]
        return processes(browser_pid, desktop_pid)

    def delayed_measure(owner: Running) -> Result[Measurement]:
        clock[0] += 2 * delay[0]
        service_cpu[0] = clock[0] - 10
        return measure(owner)

    monkeypatch.setattr(cpu, "processes", delayed_processes)
    monkeypatch.setattr(service_runtime, "measure", delayed_measure)
    sampler = accepted(Sampler.open(1, running, origin=10))
    for seconds in (1.0, 3.0):
        delay[0] = seconds
        clock[0] += 1
        sample = accepted(sampler.read())
        assert sample.cpu["chromium"].percent == pytest.approx(100)
        assert sample.cpu["bridge"].percent == pytest.approx(100)


def test_read_decreased_cpu_and_unavailable_status_are_errors(
    counters: tuple[Running, list[float], list[float]], monkeypatch: pytest.MonkeyPatch
) -> None:
    running, clock, service_cpu = counters
    sampler = accepted(Sampler.open(1, running, 2, origin=10))
    clock[0] = 11
    service_cpu[0] = 0
    assert isinstance(sampler.read(), Rejected)

    def gone(owner: Running) -> Result[Measurement]:
        assert owner == running
        return Rejected(errno.ESRCH, "service exited")

    monkeypatch.setattr(service_runtime, "measure", gone)
    assert isinstance(sampler.read(), Rejected)


def test_open_invalid_workload_pid_is_an_error(
    counters: tuple[Running, list[float], list[float]],
) -> None:
    running, _, _ = counters
    assert isinstance(Sampler.open(0, running, 2, origin=10), Rejected)


def test_read_without_compositor_keeps_decode_counters(
    counters: tuple[Running, list[float], list[float]],
) -> None:
    running, clock, service_cpu = counters
    sampler = accepted(Sampler.open(1, running, origin=10))
    clock[0] = 11
    service_cpu[0] = 1.25
    sample = accepted(sampler.read())
    assert "desktop-compositor" not in sample.cpu
    rate = sample.cpu["chromium"].percent
    assert sample.cpu["bridge"].percent == 25 and rate is not None and rate > 0
    assert sample.gpu_percent == 25 and sample.mpp_completed_window_load == {
        "core0": 12
    }


def test_read_reports_the_interval_of_each_cpu_delta(
    counters: tuple[Running, list[float], list[float]],
) -> None:
    running, clock, service_cpu = counters
    sampler = accepted(Sampler.open(1, running, origin=10))
    clock[0] = 11
    service_cpu[0] = 2
    first = accepted(sampler.read())
    clock[0] = 20
    second = accepted(sampler.read())
    assert (
        first.cpu["bridge"].interval_start,
        first.elapsed,
        first.cpu["bridge"].percent,
    ) == (0, 1, 100)
    assert (
        second.cpu["bridge"].interval_start,
        second.elapsed,
        second.cpu["bridge"].percent,
    ) == (1, 10, 0)


@pytest.mark.parametrize("pid", [1, 2], ids=["browser", "compositor"])
@pytest.mark.parametrize("replaced", [False, True], ids=["missing", "reused-pid"])
def test_read_rejects_lost_workload_identity(
    counters: tuple[Running, list[float], list[float]],
    monkeypatch: pytest.MonkeyPatch,
    pid: int,
    replaced: bool,
) -> None:
    running, clock, _ = counters
    sampler = accepted(Sampler.open(1, running, 2, origin=10))
    provider = cpu.processes

    def changed(browser_pid: int, desktop_pid: int | None) -> Result[Processes]:
        values = accepted(provider(browser_pid, desktop_pid))
        if replaced:
            values[pid] = Process(
                parent=0, start=2, ticks=0, name="chromium", kind="browser"
            )
        else:
            del values[pid]
        return Ok(values)

    monkeypatch.setattr(cpu, "processes", changed)
    clock[0] = 11
    result = sampler.read()
    assert isinstance(result, Rejected) and result.code == errno.ESRCH
    # A rejected snapshot must not advance the accepted baseline.
    monkeypatch.setattr(cpu, "processes", provider)
    clock[0] = 12
    sample = accepted(sampler.read())
    assert (sample.cpu["bridge"].interval_start, sample.elapsed) == (0, 2)


def test_read_tree_churn_retains_gap_then_recovers_from_next_snapshot(
    counters: tuple[Running, list[float], list[float]], monkeypatch: pytest.MonkeyPatch
) -> None:
    running, clock, service_cpu = counters
    sampler = accepted(Sampler.open(1, running, 2, origin=10))
    provider = cpu.processes

    def added_worker(browser_pid: int, desktop_pid: int | None) -> Result[Processes]:
        values = accepted(provider(browser_pid, desktop_pid))
        values[4] = Process(
            parent=1,
            start=2,
            ticks=int((clock[0] - 10) * 100),
            name="renderer",
            kind="renderer",
        )
        return Ok(values)

    monkeypatch.setattr(cpu, "processes", added_worker)
    clock[0], service_cpu[0] = 11, 1.25
    changed = accepted(sampler.read())
    assert changed.cpu["chromium"].percent is None
    assert changed.cpu["bridge"].percent == 25
    assert changed.cpu["desktop-compositor"].percent == 0
    clock[0], service_cpu[0] = 12, 1.50
    stable = accepted(sampler.read())
    rate = stable.cpu["chromium"].percent
    assert stable.cpu["chromium"].interval_start == 1
    assert rate is not None and rate > 0
