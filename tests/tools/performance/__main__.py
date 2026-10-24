"""module: performance; responsibility: execute standalone video measurements.

operations: main; entities: Entry, Index; tests: cli_tests and direct runs.
No pytest fixtures, collection, assertions or performance pass/fail budgets.
"""

import errno
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Literal

from pydantic import BaseModel, Field

from tests.support.outcome import Ok, Rejected, Result
from tests.support.service_process import Lease
from tests.support.service_runtime import ENDPOINT, locate
from tests.tools.performance.capture import record
from tests.tools.performance.controls import Controls
from tests.tools.performance.options import Options, parse
from tests.tools.performance.playback import FrameQuality
from tests.tools.performance.report import Utilization

_ROOT = Path(__file__).resolve().parents[3]
_CONTROL = Path("/dev/v4l2-rkmpp-control")
_HELP = """Video playback performance measurement (normal user, existing desktop session).
Usage: uv run python -m tests.tools.performance [options]
  --codec h264|vp9|av1   Select presets; repeat to choose several, default all three.
  --video PATH          Measure a custom browser-compatible video instead of presets.
  --rounds N            Repeat each workload (default 1).
  --duration SECONDS    Observation limit; omit to play the complete file.
  --warmup SECONDS      Exclude startup from statistics (default 5).
  --interval-ms MS      CPU/GPU/MPP sampling period (default 1000).
  --gpu-min-hz HZ       Request GPU minimum; omit to keep the system policy.
  --output DIRECTORY    New persistent output directory (default target/benchmarks/<time>).
  --compositor-pid PID  Optionally sample this desktop compositor process; omitted by default.
  --service PATH        Service executable used only if no valid service is running.
  --help                Show this help.
"""


class Entry(BaseModel):
    workload: str
    round: int
    report: Path
    utilization: Utilization | None = None
    frame_quality: FrameQuality | None = None


class Index(BaseModel):
    schema_version: Literal[5] = 5
    started_at: str
    options: Options
    entries: list[Entry] = Field(default_factory=list)
    failure: Rejected | None = None
    controls_restored: bool = False
    service_released: bool = False


def main(arguments: tuple[str, ...]) -> int:
    if arguments == ("--help",):
        print(_HELP)
        return 0
    options = parse(arguments, _ROOT)
    if isinstance(options, Rejected):
        print(options.operation, file=sys.stderr)
        return 2
    result = _run(options.value)
    if isinstance(result, Rejected):
        print(result.operation, file=sys.stderr)
        return 130 if result.code == errno.ECANCELED else 1
    print(f"Reports: {options.value.output / 'summary.json'}")
    return 0


def _run(options: Options) -> Result[None]:
    try:
        options.output.mkdir(parents=True)
    except OSError as error:
        return Rejected(error.errno or 0, f"create new report directory: {error}")
    index = Index(started_at=datetime.now(UTC).isoformat(), options=options)
    lease: Lease | None = None
    controls: Controls | None = None
    save_failure: Rejected | None = None
    try:
        running = locate()
        if isinstance(running, Rejected):
            index.failure = running
        else:
            acquired = (
                Lease.borrow(running.value)
                if running.value is not None
                else Lease.start(
                    options.service_binary,
                    _CONTROL,
                    ENDPOINT,
                    options.output / "service.log",
                )
            )
            if isinstance(acquired, Rejected):
                index.failure = acquired
            else:
                lease = acquired.value
                configured = Controls.open(options.gpu_minimum_hz, options.interval_ms)
                if isinstance(configured, Rejected):
                    index.failure = configured
                else:
                    controls = configured.value
                    index.failure = _workloads(options, lease, controls, index)
    except KeyboardInterrupt:
        index.failure = Rejected(errno.ECANCELED, "measurement interrupted")
    finally:
        if controls is not None:
            restored = controls.close()
            index.controls_restored = isinstance(restored, Ok)
            if isinstance(restored, Rejected):
                index.failure = restored
        if lease is not None:
            released = lease.close()
            index.service_released = isinstance(released, Ok)
            if isinstance(released, Rejected):
                index.failure = released
        try:
            _ = (options.output / "summary.json").write_text(
                index.model_dump_json(indent=2)
            )
        except OSError as error:
            save_failure = Rejected(
                error.errno or 0, f"save measurement index: {error}"
            )
    if save_failure is not None:
        return save_failure
    return index.failure if index.failure is not None else Ok(None)


def _workloads(
    options: Options, lease: Lease, controls: Controls, index: Index
) -> Rejected | None:
    for ordinal, workload in enumerate(options.workloads):
        for round in range(1, options.rounds + 1):
            directory = options.output / f"{ordinal + 1}-{workload.name}-{round}"
            entry = Entry(
                workload=workload.name, round=round, report=directory / "report.json"
            )
            index.entries.append(entry)
            print(
                f"Measuring {workload.name}, round {round}/{options.rounds}", flush=True
            )
            measured = record(
                workload, round, directory, options, lease.running, controls.settings
            )
            if isinstance(measured, Rejected):
                return measured
            entry.utilization = measured.value.utilization
            entry.frame_quality = measured.value.frame_quality
            usage = measured.value.utilization
            if usage is not None and entry.frame_quality is not None:
                print(
                    f"  CPU Chromium={usage.cpu['chromium'].mean_percent:.2f}% service={usage.cpu['bridge'].mean_percent:.2f}% GPU={usage.gpu.mean:.2f}% steady-dropped={entry.frame_quality.dropped_percent:.2f}%",
                    flush=True,
                )
            presentation = measured.value.presentation
            if presentation is not None and presentation.availability != "available":
                print(
                    f"  Presentation intervals unavailable: {presentation.availability} (background/covered window)",
                    flush=True,
                )
    return None


if __name__ == "__main__":
    sys.exit(main(tuple(sys.argv[1:])))
