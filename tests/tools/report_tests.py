"""module: report_tests; responsibility: define utilization summaries.

operations: pytest cases; entities: none; tests: distributions, warmup, shape.
No browser, service or hardware is started by these statistical contracts.
"""

import math

import pytest

from tests.support.outcome import Rejected, accepted
from tests.tools.report import Sample, describe, summarize


def test_describe_preserves_mean_extremes_and_interpolated_quantiles() -> None:
    statistic = accepted(describe((10.0, 20.0)))
    assert (statistic.mean, statistic.minimum, statistic.maximum, statistic.p50) == (
        15,
        10,
        20,
        15,
    )
    assert statistic.p95 == pytest.approx(19.5)
    assert statistic.p99 == pytest.approx(19.9)


@pytest.mark.parametrize("values", [(), (math.nan,), (math.inf,)])
def test_describe_invalid_samples_return_error(values: tuple[float, ...]) -> None:
    assert isinstance(describe(values), Rejected)


def test_summarize_excludes_warmup_and_keeps_each_process_and_mpp_core() -> None:
    samples = tuple(
        Sample(
            elapsed=elapsed,
            cpu={"chromium": value, "bridge": value / 10},
            gpu_percent=value,
            gpu_frequency=1000000000,
            mpp={"core0": value / 2},
        )
        for elapsed, value in ((1, 100), (5, 10), (6, 20))
    )
    summary = accepted(summarize(samples, 5))
    assert summary.samples == 2 and summary.cpu["chromium"].mean == 15
    assert summary.cpu["bridge"].mean == 1.5 and summary.gpu.mean == 15
    assert summary.mpp["core0"].mean == 7.5


def test_summarize_empty_steady_window_and_inconsistent_counter_sets_fail() -> None:
    first = Sample(
        elapsed=1, cpu={"chromium": 1}, gpu_percent=1, gpu_frequency=1, mpp={"core": 1}
    )
    second = first.model_copy(update={"elapsed": 2, "cpu": {"bridge": 1}})
    assert isinstance(summarize((first,), 5), Rejected)
    assert isinstance(summarize((first, second), 0), Rejected)
