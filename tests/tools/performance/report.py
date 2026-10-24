"""module: report; responsibility: summarize measured utilization distributions.

operations: CpuSample, Sample, Distribution, CpuUsage, Utilization, describe, summarize;
entities: five;
tests: report_tests. The sampler implements this consumer-owned Sample contract.
"""

import errno
import math
from collections.abc import Sequence
from statistics import mean
from typing import Literal

from pydantic import BaseModel

from tests.support.outcome import Ok, Rejected, Result


class CpuSample(BaseModel, frozen=True):
    interval_start: float
    elapsed: float
    # None is an interval whose process identity set changed; no complete
    # CPU delta exists. It is retained as missing data, never as idle time.
    percent: float | None


class Sample(BaseModel):
    elapsed: float
    gpu_percent: float
    gpu_frequency: int
    mpp_completed_window_load: dict[str, float]
    cpu: dict[str, CpuSample]


class Distribution(BaseModel, frozen=True):
    mean: float
    minimum: float
    maximum: float
    p50: float
    p95: float
    p99: float


class CpuUsage(BaseModel, frozen=True):
    # Average CPU time per observed wall time; distribution describes the
    # individual interval rates and intentionally keeps sample percentiles.
    mean_percent: float
    observed_seconds: float
    samples: int
    missing_seconds: float
    missing_samples: int
    interval_distribution: Distribution


class Utilization(BaseModel, frozen=True):
    samples: int
    cpu: dict[str, CpuUsage]
    gpu: Distribution
    gpu_frequency: Distribution
    mpp_completed_window_load: dict[str, Distribution]
    mpp_semantics: Literal["last_completed_driver_window_without_freshness"] = (
        "last_completed_driver_window_without_freshness"
    )


def describe(values: Sequence[float]) -> Result[Distribution]:
    if not values or any(not math.isfinite(value) for value in values):
        return Rejected(errno.EINVAL, "distribution needs finite, nonempty samples")
    ordered = sorted(values)

    def quantile(fraction: float) -> float:
        position = (len(ordered) - 1) * fraction
        lower = math.floor(position)
        upper = math.ceil(position)
        weight = position - lower
        left, right = ordered[lower], ordered[upper]
        # Opposite signs can overflow the difference even when both endpoints
        # and their interpolated quantile are finite.
        if left < 0 < right:
            return left * (1 - weight) + right * weight
        return left + (right - left) * weight

    return Ok(
        Distribution(
            mean=mean(ordered),
            minimum=ordered[0],
            maximum=ordered[-1],
            p50=quantile(0.5),
            p95=quantile(0.95),
            p99=quantile(0.99),
        )
    )


def summarize(samples: Sequence[Sample], warmup_seconds: float) -> Result[Utilization]:
    if warmup_seconds < 0 or not math.isfinite(warmup_seconds):
        return Rejected(errno.EINVAL, "invalid summary warmup")
    previous_end: dict[str, float] = {}
    previous_sample = 0.0
    for sample in samples:
        if not math.isfinite(sample.elapsed) or sample.elapsed < previous_sample:
            return Rejected(errno.EINVAL, "invalid sample time")
        previous_sample = sample.elapsed
        for key, counter in sample.cpu.items():
            if (
                not math.isfinite(counter.interval_start)
                or not math.isfinite(counter.elapsed)
                or counter.interval_start < previous_end.get(key, 0.0)
                or counter.elapsed <= counter.interval_start
                or counter.elapsed > sample.elapsed
            ):
                return Rejected(errno.EINVAL, "invalid or overlapping CPU interval")
            previous_end[key] = counter.elapsed
    steady = [sample for sample in samples if sample.elapsed >= warmup_seconds]
    # CPU rates describe intervals. Do not invent the unobserved fraction
    # of a rate straddling warmup; GPU/MPP records remain point observations.
    if not steady:
        return Rejected(errno.ENODATA, "no post-warmup samples")
    cpu_keys, mpp_keys = set(steady[0].cpu), set(steady[0].mpp_completed_window_load)
    if any(
        set(sample.cpu) != cpu_keys or set(sample.mpp_completed_window_load) != mpp_keys
        for sample in steady
    ):
        return Rejected(
            errno.EPROTO, "counter identities changed within the measurement"
        )
    cpu: dict[str, CpuUsage] = {}
    mpp: dict[str, Distribution] = {}
    for key in sorted(cpu_keys):
        intervals = [
            sample.cpu[key]
            for sample in steady
            if sample.cpu[key].interval_start >= warmup_seconds
        ]
        counters = [counter for counter in intervals if counter.percent is not None]
        if not counters:
            return Rejected(errno.ENODATA, "no complete post-warmup CPU interval")
        observed = sum(counter.elapsed - counter.interval_start for counter in counters)
        result = describe(
            [counter.percent for counter in counters if counter.percent is not None]
        )
        if isinstance(result, Rejected):
            return result
        cpu[key] = CpuUsage(
            mean_percent=sum(
                counter.percent
                * ((counter.elapsed - counter.interval_start) / observed)
                for counter in counters
                if counter.percent is not None
            ),
            observed_seconds=observed,
            samples=len(counters),
            missing_seconds=sum(
                counter.elapsed - counter.interval_start
                for counter in intervals
                if counter.percent is None
            ),
            missing_samples=len(intervals) - len(counters),
            interval_distribution=result.value,
        )
    for key in sorted(mpp_keys):
        result = describe([sample.mpp_completed_window_load[key] for sample in steady])
        if isinstance(result, Rejected):
            return result
        mpp[key] = result.value
    gpu = describe([sample.gpu_percent for sample in steady])
    if isinstance(gpu, Rejected):
        return gpu
    try:
        frequencies = [float(sample.gpu_frequency) for sample in steady]
    except OverflowError:
        return Rejected(errno.EINVAL, "GPU frequency exceeds floating-point range")
    frequency = describe(frequencies)
    if isinstance(frequency, Rejected):
        return frequency
    return Ok(
        Utilization(
            samples=len(steady),
            cpu=cpu,
            gpu=gpu.value,
            gpu_frequency=frequency.value,
            mpp_completed_window_load=mpp,
        )
    )
