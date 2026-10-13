"""module: options; responsibility: validate standalone measurement inputs.

operations: Expected, Workload, Options, parse; entities: three;
tests: options_tests. Preset data belongs here, never in binary artifacts.
"""

import errno
import time
from pathlib import Path

from pydantic import BaseModel, Field, ValidationError

from tests.support.outcome import Ok, Rejected, Result


class Expected(BaseModel, frozen=True):
    width: int
    height: int
    frames: int


class Workload(BaseModel, frozen=True):
    name: str
    source: Path
    expected: Expected | None = None
    expand_frames: int | None = Field(default=None, gt=0)


class Options(BaseModel, frozen=True):
    workloads: tuple[Workload, ...]
    rounds: int = Field(gt=0)
    gpu_minimum_hz: int | None
    interval_ms: int = Field(gt=0)
    duration_seconds: float | None = Field(default=None, gt=0, allow_inf_nan=False)
    warmup_seconds: float = Field(ge=0, allow_inf_nan=False)
    output: Path
    service_binary: Path


_PRESETS = {
    "h264": "h264-4k30-seed.mp4",
    "vp9": "vp9-10-4k30-seed.webm",
    "av1": "av1-10-4k30-seed.webm",
}
_SINGLE = {
    "--rounds",
    "--gpu-min-hz",
    "--interval-ms",
    "--duration",
    "--warmup",
    "--output",
    "--video",
    "--service",
}


def parse(arguments: tuple[str, ...], root: Path) -> Result[Options]:
    values: dict[str, str] = {}
    codecs: list[str] = []
    if len(arguments) % 2:
        return Rejected(errno.EINVAL, "each measurement option requires a value")
    for index in range(0, len(arguments), 2):
        name, value = arguments[index : index + 2]
        if name == "--codec":
            if value not in _PRESETS or value in codecs:
                return Rejected(errno.EINVAL, "choose unique --codec h264, vp9 or av1")
            codecs.append(value)
        elif name in _SINGLE and name not in values:
            values[name] = value
        else:
            return Rejected(
                errno.EINVAL, f"unknown or repeated measurement option: {name}"
            )
    if "--video" in values and codecs:
        return Rejected(errno.EINVAL, "--video and --codec cannot be combined")
    try:
        rounds = int(values.get("--rounds", "1"))
        interval = int(values.get("--interval-ms", "1000"))
        minimum = int(values["--gpu-min-hz"]) if "--gpu-min-hz" in values else None
        duration = float(values["--duration"]) if "--duration" in values else None
        warmup = float(values.get("--warmup", "5"))
    except ValueError:
        return Rejected(errno.EINVAL, "invalid numeric measurement option")
    if (
        minimum is not None
        and minimum <= 0
        or duration is not None
        and warmup >= duration
    ):
        return Rejected(errno.EINVAL, "invalid frequency or steady-state interval")
    workloads = (
        (Workload(name="video", source=Path(values["--video"]).resolve()),)
        if "--video" in values
        else tuple(
            Workload(
                name=codec,
                source=root / "tests/artifacts" / _PRESETS[codec],
                expected=Expected(width=3840, height=2160, frames=900),
                expand_frames=900,
            )
            for codec in codecs or tuple(_PRESETS)
        )
    )
    for workload in workloads:
        if not workload.source.is_file():
            return Rejected(
                errno.ENOENT, f"missing measurement video: {workload.source}"
            )
    stamp = (
        time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
        + f"-{time.time_ns() % 1000000000:09d}"
    )
    try:
        options = Options(
            workloads=workloads,
            rounds=rounds,
            gpu_minimum_hz=minimum,
            interval_ms=interval,
            duration_seconds=duration,
            warmup_seconds=warmup,
            output=Path(values["--output"]).resolve()
            if "--output" in values
            else root / "target/benchmarks" / stamp,
            service_binary=Path(values["--service"]).resolve()
            if "--service" in values
            else root / "target/release/v4l2-stateful-rkmpp",
        )
    except ValidationError as error:
        return Rejected(errno.EINVAL, f"invalid measurement options: {error}")
    return Ok(options)
