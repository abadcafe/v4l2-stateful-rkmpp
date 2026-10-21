"""module: report_tests; responsibility: define utilization summaries.

operations: pytest cases; entities: none; tests: distributions, warmup, shape.
No browser, service or hardware is started by these statistical contracts.
"""

import math

import pytest

from tests.support.outcome import Rejected, accepted
from tests.tools.performance.report import Sample, describe, summarize


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


def test_describe_opposite_finite_extremes_have_finite_quantiles() -> None:
    statistic = accepted(describe((-1e308, 1e308)))
    assert statistic.mean == 0 and statistic.p50 == 0
    assert statistic.p95 == pytest.approx(9e307)
    assert statistic.p99 == pytest.approx(9.8e307)


@pytest.mark.parametrize("values", [(), (math.nan,), (math.inf,)])
def test_describe_invalid_samples_return_error(values: tuple[float, ...]) -> None:
    assert isinstance(describe(values), Rejected)


def test_summarize_excludes_warmup_and_keeps_each_process_and_mpp_core() -> None:
    samples = tuple(
        Sample(
            interval_start=start,
            elapsed=elapsed,
            cpu={"chromium": value, "bridge": value / 10},
            gpu_percent=value,
            gpu_frequency=1000000000,
            mpp_completed_window_load={"core0": value / 2},
        )
        for start, elapsed, value in ((0, 1, 100), (1, 5, 10), (5, 6, 20))
    )
    summary = accepted(summarize(samples, 5))
    assert summary.samples == 2 and summary.cpu["chromium"].mean_percent == 20
    assert summary.cpu["bridge"].mean_percent == 2 and summary.gpu.mean == 15
    assert summary.mpp_completed_window_load["core0"].mean == 7.5


def test_summarize_empty_steady_window_and_inconsistent_counter_sets_fail() -> None:
    first = Sample(
        interval_start=0,
        elapsed=1,
        cpu={"chromium": 1},
        gpu_percent=1,
        gpu_frequency=1,
        mpp_completed_window_load={"core": 1},
    )
    second = first.model_copy(
        update={"interval_start": 1, "elapsed": 2, "cpu": {"bridge": 1}}
    )
    assert isinstance(summarize((first,), 5), Rejected)
    assert isinstance(summarize((first, second), 0), Rejected)


def test_summarize_labels_mpp_values_without_inventing_freshness() -> None:
    # mpp_common.c::mpp_dev_load updates only on task completion. The
    # proc ABI has no timestamp/counter to distinguish an idle stale
    # value from a genuinely constant load. Do not claim realtime usage.
    first = Sample(
        interval_start=0,
        elapsed=1,
        cpu={"bridge": 1},
        gpu_percent=1,
        gpu_frequency=1,
        mpp_completed_window_load={"core": 42},
    )
    second = first.model_copy(update={"interval_start": 1, "elapsed": 2})
    summary = accepted(summarize((first, second), 0))
    assert summary.mpp_semantics == "last_completed_driver_window_without_freshness"
    assert summary.mpp_completed_window_load["core"].mean == 42
    assert summary.model_dump()["mpp_semantics"] == summary.mpp_semantics


def test_summarize_cpu_average_accounts_for_unequal_sample_intervals() -> None:
    # One CPU-second in ten wall seconds is 10%, not the arithmetic
    # average (50%) of the one-second busy and nine-second idle rates.
    samples = tuple(
        Sample(
            interval_start=start,
            elapsed=elapsed,
            cpu={"bridge": value},
            gpu_percent=0,
            gpu_frequency=1000000000,
            mpp_completed_window_load={"core": 0},
        )
        for start, elapsed, value in ((0, 1, 100), (1, 10, 0))
    )
    summary = accepted(summarize(samples, 0))
    assert summary.cpu["bridge"].mean_percent == pytest.approx(10)

    assert summary.cpu["bridge"].observed_seconds == 10
    assert summary.cpu["bridge"].samples == 2
    assert summary.cpu["bridge"].interval_distribution.mean == 50


@pytest.mark.parametrize(
    ("start", "end"),
    [(0, 0), (2, 1), (-1, 1), (math.nan, 1), (0, math.nan), (0, math.inf)],
)
def test_summarize_rejects_invalid_cpu_interval(start: float, end: float) -> None:
    sample = Sample(
        interval_start=start,
        elapsed=end,
        cpu={"bridge": 1},
        gpu_percent=1,
        gpu_frequency=1,
        mpp_completed_window_load={"core": 1},
    )
    assert isinstance(summarize((sample,), 0), Rejected)


def test_summarize_rejects_overlapping_intervals() -> None:
    sample = Sample(
        interval_start=0,
        elapsed=2,
        cpu={"bridge": 1},
        gpu_percent=1,
        gpu_frequency=1,
        mpp_completed_window_load={"core": 1},
    )
    overlap = sample.model_copy(update={"interval_start": 1, "elapsed": 3})
    assert isinstance(summarize((sample, overlap), 0), Rejected)


def test_summarize_requires_complete_post_warmup_cpu_interval() -> None:
    sample = Sample(
        interval_start=0,
        elapsed=10,
        cpu={"bridge": 50},
        gpu_percent=1,
        gpu_frequency=1,
        mpp_completed_window_load={"core": 1},
    )
    # A rate for [0, 10] cannot reveal the rate in the subinterval [5, 10].
    assert isinstance(summarize((sample,), 5), Rejected)


def test_summarize_cpu_average_does_not_assign_load_to_unsampled_gaps() -> None:
    first = Sample(
        interval_start=0,
        elapsed=1,
        cpu={"bridge": 100},
        gpu_percent=1,
        gpu_frequency=1,
        mpp_completed_window_load={"core": 1},
    )
    second = first.model_copy(
        update={"interval_start": 9, "elapsed": 10, "cpu": {"bridge": 0}}
    )
    summary = accepted(summarize((first, second), 0))
    assert summary.cpu["bridge"].observed_seconds == 2
    assert summary.cpu["bridge"].mean_percent == 50
