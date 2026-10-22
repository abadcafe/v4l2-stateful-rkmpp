"""module: report; responsibility: summarize measured utilization distributions.

operations: Sample, Distribution, CpuUsage, Utilization, describe, summarize;
entities: four;
tests: report_tests. The sampler implements this consumer-owned Sample contract.
"""

import errno
import math
from collections.abc import Sequence
from statistics import mean
from typing import Literal

from pydantic import BaseModel

from tests.support.outcome import Ok, Rejected, Result


class Sample(BaseModel):
    interval_start: float
    elapsed: float
    gpu_percent: float
    gpu_frequency: int
    mpp_completed_window_load: dict[str, float]
    cpu: dict[str, float]


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
    previous_end = 0.0
    for sample in samples:
        if (
            not math.isfinite(sample.interval_start)
            or not math.isfinite(sample.elapsed)
            or sample.interval_start < previous_end
            or sample.elapsed <= sample.interval_start
        ):
            return Rejected(errno.EINVAL, "invalid or overlapping sample interval")
        previous_end = sample.elapsed
    steady = [sample for sample in samples if sample.elapsed >= warmup_seconds]
    # CPU rates describe intervals. Do not invent the unobserved fraction
    # of a rate straddling warmup; GPU/MPP records remain point observations.
    cpu_steady = [
        sample for sample in steady if sample.interval_start >= warmup_seconds
    ]
    if not cpu_steady:
        return Rejected(errno.ENODATA, "no complete post-warmup CPU interval")
    cpu_keys, mpp_keys = set(steady[0].cpu), set(steady[0].mpp_completed_window_load)
    if any(
        set(sample.cpu) != cpu_keys or set(sample.mpp_completed_window_load) != mpp_keys
        for sample in steady
    ):
        return Rejected(
            errno.EPROTO, "counter identities changed within the measurement"
        )
    cpu: dict[str, CpuUsage] = {}
    observed = sum(sample.elapsed - sample.interval_start for sample in cpu_steady)
    mpp: dict[str, Distribution] = {}
    for key in sorted(cpu_keys):
        result = describe([sample.cpu[key] for sample in cpu_steady])
        if isinstance(result, Rejected):
            return result
        cpu[key] = CpuUsage(
            mean_percent=sum(
                sample.cpu[key] * ((sample.elapsed - sample.interval_start) / observed)
                for sample in cpu_steady
            ),
            observed_seconds=observed,
            samples=len(cpu_steady),
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
