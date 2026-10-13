"""module: tests.tools.telemetry_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_gpu_load_parses_percentage_and_frequency
    test_gpu_load_malformed_or_out_of_range_is_rejected
    test_mpp_load_preserves_every_core_percentage
    test_mpp_load_missing_or_invalid_counters_are_rejected

Hardware counters use the published sysfs/proc formats.
"""

import pytest

from tests.support.outcome import Rejected, accepted
from tests.tools.telemetry import gpu_load, mpp_load


def test_gpu_load_parses_percentage_and_frequency() -> None:
    assert accepted(gpu_load("37@800000000Hz\n")) == (37.0, 800000000)


@pytest.mark.parametrize("raw", ["", "101@10Hz", "-1@10Hz", "load: 1", "1@0Hz"])
def test_gpu_load_malformed_or_out_of_range_is_rejected(raw: str) -> None:
    assert isinstance(gpu_load(raw), Rejected)


def test_mpp_load_preserves_every_core_percentage() -> None:
    raw = "fdc38100.rkvdec-core load: 17.50% utilization: 20.00%\nfdc70000.av1d load: 3.00% utilization: 4.00%\n"
    assert accepted(mpp_load(raw)) == {
        "fdc38100.rkvdec-core": 17.5,
        "fdc70000.av1d": 3.0,
    }


@pytest.mark.parametrize(
    "raw",
    [
        "",
        "please set load_interval first!!!",
        "fdc70000.av1d load: 101.00% utilization: 0.00%",
    ],
)
def test_mpp_load_missing_or_invalid_counters_are_rejected(raw: str) -> None:
    assert isinstance(mpp_load(raw), Rejected)
