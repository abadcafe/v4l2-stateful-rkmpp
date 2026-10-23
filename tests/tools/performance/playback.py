"""module: playback; responsibility: summarize observed video frame counters.

operations: Observation, FrameQuality, summarize; entities: two;
tests: playback_tests. Only complete post-warmup observation windows contribute;
missing frame progress is unavailable data, never zero percent dropped frames.
"""

import errno
import math
from collections.abc import Sequence

from pydantic import BaseModel

from tests.support.outcome import Ok, Rejected, Result
from tests.support.player import Playback


class Observation(BaseModel):
    elapsed: float
    playback: Playback


class FrameQuality(BaseModel, frozen=True):
    start_seconds: float
    end_seconds: float
    total: int
    dropped: int
    dropped_percent: float


def summarize(
    observations: Sequence[Observation], warmup_seconds: float
) -> Result[FrameQuality]:
    if warmup_seconds < 0 or not math.isfinite(warmup_seconds):
        return Rejected(errno.EINVAL, "invalid frame quality warmup")
    first: Observation | None = None
    previous: Observation | None = None
    for observation in observations:
        if (
            not math.isfinite(observation.elapsed)
            or observation.elapsed < 0
            or previous is not None
            and observation.elapsed <= previous.elapsed
        ):
            return Rejected(errno.EINVAL, "invalid frame observation time")
        frames = observation.playback.frames
        if not 0 <= frames.dropped <= frames.total:
            return Rejected(errno.EPROTO, "invalid video frame counters")
        if previous is not None:
            old = previous.playback.frames
            total, dropped = frames.total - old.total, frames.dropped - old.dropped
            # The Media Playback Quality counters advance together for a
            # dropped frame. A reset or impossible delta invalidates the
            # observation window rather than manufacturing a drop rate.
            if total < 0 or not 0 <= dropped <= total:
                return Rejected(errno.EPROTO, "video frame counter lifetime changed")
        if first is None and observation.elapsed >= warmup_seconds:
            first = observation
        previous = observation
    if first is None or previous is None or first is previous:
        return Rejected(errno.ENODATA, "no complete post-warmup frame interval")
    total = previous.playback.frames.total - first.playback.frames.total
    dropped = previous.playback.frames.dropped - first.playback.frames.dropped
    if total == 0:
        return Rejected(errno.ENODATA, "no frames in the post-warmup interval")
    return Ok(
        FrameQuality(
            start_seconds=first.elapsed,
            end_seconds=previous.elapsed,
            total=total,
            dropped=dropped,
            dropped_percent=100 * dropped / total,
        )
    )
