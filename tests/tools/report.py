"""module: report; responsibility: summarize measured utilization distributions.

operations: Sample, Distribution, Utilization, describe, summarize; entities: three;
tests: report_tests. The sampler implements this consumer-owned Sample contract.
"""

import errno
import math
from collections.abc import Sequence
from statistics import mean

from pydantic import BaseModel

from tests.support.outcome import Ok, Rejected, Result


class Sample(BaseModel):
    elapsed: float
    gpu_percent: float
    gpu_frequency: int
    mpp: dict[str, float]
    cpu: dict[str, float]


class Distribution(BaseModel, frozen=True):
    mean: float
    minimum: float
    maximum: float
    p50: float
    p95: float
    p99: float


class Utilization(BaseModel, frozen=True):
    samples: int
    cpu: dict[str, Distribution]
    gpu: Distribution
    gpu_frequency: Distribution
    mpp: dict[str, Distribution]


def describe(values: Sequence[float]) -> Result[Distribution]:
    if not values or any(not math.isfinite(value) for value in values):
        return Rejected(errno.EINVAL, "distribution needs finite, nonempty samples")
    ordered = sorted(values)

    def quantile(fraction: float) -> float:
        position = (len(ordered) - 1) * fraction
        lower = math.floor(position)
        upper = math.ceil(position)
        return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)

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
    steady = [sample for sample in samples if sample.elapsed >= warmup_seconds]
    if not steady:
        return Rejected(
            errno.ENODATA,
            "no steady-state samples; choose a longer observation or shorter --warmup",
        )
    cpu_keys, mpp_keys = set(steady[0].cpu), set(steady[0].mpp)
    if any(
        set(sample.cpu) != cpu_keys or set(sample.mpp) != mpp_keys for sample in steady
    ):
        return Rejected(
            errno.EPROTO, "counter identities changed within the measurement"
        )
    cpu: dict[str, Distribution] = {}
    mpp: dict[str, Distribution] = {}
    for key in sorted(cpu_keys):
        result = describe([sample.cpu[key] for sample in steady])
        if isinstance(result, Rejected):
            return result
        cpu[key] = result.value
    for key in sorted(mpp_keys):
        result = describe([sample.mpp[key] for sample in steady])
        if isinstance(result, Rejected):
            return result
        mpp[key] = result.value
    gpu = describe([sample.gpu_percent for sample in steady])
    frequency = describe([float(sample.gpu_frequency) for sample in steady])
    if isinstance(gpu, Rejected):
        return gpu
    if isinstance(frequency, Rejected):
        return frequency
    return Ok(
        Utilization(
            samples=len(steady),
            cpu=cpu,
            gpu=gpu.value,
            gpu_frequency=frequency.value,
            mpp=mpp,
        )
    )
