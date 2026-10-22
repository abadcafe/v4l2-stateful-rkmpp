"""module: tests.tools.performance.telemetry_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_gpu_load_parses_percentage_and_frequency
    test_gpu_load_malformed_or_out_of_range_is_rejected
    test_mpp_completed_window_load_preserves_every_core_percentage
    test_mpp_completed_window_load_missing_or_invalid_counters_are_rejected

Hardware counters use the published sysfs/proc formats.
"""

import errno
import sys

import pytest

from tests.support.outcome import Rejected, accepted
from tests.tools.performance.telemetry import gpu_load, mpp_completed_window_load


def test_gpu_load_parses_percentage_and_frequency() -> None:
    assert accepted(gpu_load("37@800000000Hz\n")) == (37.0, 800000000)


@pytest.mark.parametrize("raw", ["", "101@10Hz", "-1@10Hz", "load: 1", "1@0Hz"])
def test_gpu_load_malformed_or_out_of_range_is_rejected(raw: str) -> None:
    assert isinstance(gpu_load(raw), Rejected)


def test_gpu_load_integer_conversion_limit_returns_error_and_allows_retry() -> None:
    original = sys.get_int_max_str_digits()
    limit = sys.int_info.str_digits_check_threshold
    try:
        sys.set_int_max_str_digits(limit)
        result = gpu_load("1@" + "9" * (limit + 1) + "Hz")
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
        assert accepted(gpu_load("37@800000000Hz")) == (37.0, 800000000)
    finally:
        sys.set_int_max_str_digits(original)


def test_mpp_completed_window_load_preserves_every_core_percentage() -> None:
    raw = "fdc38100.rkvdec-core load: 17.50% utilization: 20.00%\nfdc70000.av1d load: 3.00% utilization: 4.00%\n"
    assert accepted(mpp_completed_window_load(raw)) == {
        "fdc38100.rkvdec-core": 17.5,
        "fdc70000.av1d": 3.0,
    }


@pytest.mark.parametrize(
    "raw",
    [
        "",
        "please set load_interval first!!!",
        "fdc70000.av1d load: -1.00% utilization: 0.00%",
    ],
)
def test_mpp_completed_window_load_missing_or_invalid_counters_are_rejected(
    raw: str,
) -> None:
    assert isinstance(mpp_completed_window_load(raw), Rejected)


@pytest.mark.parametrize(
    "raw",
    [
        "core load: 1.00% utilization: 1.00%\ncorrupt line\n",
        "core load: 1.00% utilization: 1.00%\ncore load: 2.00% utilization: 2.00%\n",
        "core load: 1.00% utilization: NaN%\n",
        "core load: 1.00% utilization:",
        "core load: " + "9" * 400 + ".00% utilization: 1.00%",
        "core load: 1.00% utilization: " + "9" * 400 + ".00%",
    ],
    ids=[
        "trailing-garbage",
        "duplicate-core",
        "nonfinite-utilization",
        "truncated-utilization",
        "overflowing-load",
        "overflowing-utilization",
    ],
)
def test_mpp_completed_window_load_rejects_partial_or_duplicate_records(
    raw: str,
) -> None:
    assert isinstance(mpp_completed_window_load(raw), Rejected)


def test_mpp_completed_window_load_preserves_aggregate_driver_values() -> None:
    # Vendor mpp_dev_load caps by 100 * core_count, not always 100.
    # Its last completed window can also remain stale while idle.
    # Neither a value above 100 nor an unchanged value is proof of corruption.
    assert accepted(
        mpp_completed_window_load("core load: 125.00% utilization: 110.00%\n")
    ) == {"core": 125.0}
