"""module: playback_tests; responsibility: specify observed frame counter windows.

operations: pytest cases; entities: none; tests: warmup, missing intervals,
monotonic observations, counter lifetime and exact dropped-frame deltas.
"""

import errno
import math

import pytest

from tests.support.outcome import Rejected, accepted
from tests.support.player import Box, Playback, Quality
from tests.tools.performance.playback import Observation, summarize


def observation(elapsed: float, total: int, dropped: int) -> Observation:
    return Observation(
        elapsed=elapsed,
        playback=Playback(
            time=elapsed,
            ended=False,
            paused=False,
            error=None,
            width=3840,
            height=2160,
            frames=Quality(total=total, dropped=dropped),
            hdr=False,
            visible=True,
            focused=False,
            box=Box(x=0, y=0, width=768, height=432),
        ),
    )


def test_summarize_excludes_startup_drops_and_the_interval_crossing_warmup() -> None:
    samples = (
        observation(0, 0, 0),
        observation(1, 100, 20),
        observation(2.5, 150, 30),
        observation(4, 200, 30),
    )
    result = accepted(summarize(samples, 2))
    assert (result.start_seconds, result.end_seconds) == (2.5, 4)
    assert (result.total, result.dropped, result.dropped_percent) == (50, 0, 0)


@pytest.mark.parametrize("dropped,percent", [(0, 0), (5, 10), (50, 100)])
def test_summarize_uses_post_warmup_counter_differences(
    dropped: int, percent: int
) -> None:
    result = accepted(
        summarize((observation(2, 100, 20), observation(4, 150, 20 + dropped)), 2)
    )
    assert (result.total, result.dropped, result.dropped_percent) == (
        50,
        dropped,
        percent,
    )


@pytest.mark.parametrize(
    "samples",
    [
        (),
        (observation(3, 100, 0),),
        (observation(0, 0, 0), observation(1, 100, 0)),
        (observation(1, 100, 0), observation(3, 100, 0)),
        (observation(2, 100, 0), observation(3, 100, 0)),
    ],
)
def test_summarize_missing_observed_frames_returns_no_data(
    samples: tuple[Observation, ...],
) -> None:
    result = summarize(samples, 2)
    assert isinstance(result, Rejected) and result.code == errno.ENODATA


@pytest.mark.parametrize("warmup", [-1, math.nan, math.inf])
def test_summarize_rejects_invalid_warmup(warmup: float) -> None:
    result = summarize((observation(0, 0, 0), observation(1, 30, 0)), warmup)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


@pytest.mark.parametrize("elapsed", [-1, 0, math.nan, math.inf])
def test_summarize_rejects_invalid_or_nonincreasing_observation_time(
    elapsed: float,
) -> None:
    result = summarize((observation(0, 0, 0), observation(elapsed, 30, 0)), 0)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


@pytest.mark.parametrize(
    "total,dropped", [(-1, 0), (100, -1), (100, 101), (99, 20), (110, 19), (110, 40)]
)
def test_summarize_rejects_invalid_reset_or_inconsistent_counters(
    total: int, dropped: int
) -> None:
    result = summarize((observation(0, 100, 20), observation(1, total, dropped)), 0)
    assert isinstance(result, Rejected) and result.code == errno.EPROTO
