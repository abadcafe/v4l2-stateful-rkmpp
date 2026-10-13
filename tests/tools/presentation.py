"""module: presentation; responsibility: observe browser presentation intervals.

operations: Monitor start/read, Presentation; entities: Gap, _Observed,
Presentation, Monitor; tests: presentation_tests through the public CDP protocol.
Unavailable callbacks remain missing data, never a zero latency measurement.
"""

import errno
import math
from typing import Literal

from pydantic import BaseModel, Field

from tests.support.cdp import Cdp
from tests.support.outcome import Ok, Rejected, Result
from tests.tools.report import Distribution, describe


class Gap(BaseModel, frozen=True):
    elapsed: float = Field(ge=0, allow_inf_nan=False)
    milliseconds: float = Field(ge=0, allow_inf_nan=False)


class _Observed(BaseModel):
    callbacks: int = Field(ge=0)
    frames: int = Field(ge=0)
    first_ms: float = Field(ge=0, allow_inf_nan=False)
    last_ms: float = Field(ge=0, allow_inf_nan=False)
    maximum_gap_ms: float = Field(ge=0, allow_inf_nan=False)
    media_time: float = Field(ge=0, allow_inf_nan=False)
    gaps: list[Gap]


class Presentation(_Observed):
    availability: Literal["available", "no-callbacks", "no-steady-intervals"]
    steady_gap: Distribution | None
    scope: str = "requestVideoFrameCallback observations; physical display scanout is not measured"


class Monitor:
    def __init__(self, tab: Cdp) -> None:
        self._tab: Cdp = tab

    @classmethod
    def start(cls, tab: Cdp) -> Result[Monitor]:
        installed = tab.evaluate(
            """(()=>{const v=document.querySelector('video');
            const p=window.__rkmppPresentation={callbacks:0,frames:0,first_ms:0,last_ms:0,
              maximum_gap_ms:0,media_time:0,gaps:[]};const start=performance.now();
            const observe=(now,m)=>{if(p.callbacks===0)p.first_ms=now;
              else{const gap=now-p.last_ms;p.maximum_gap_ms=Math.max(p.maximum_gap_ms,gap);
                p.gaps.push({elapsed:(now-start)/1000,milliseconds:gap});}
              p.callbacks++;p.frames=m.presentedFrames;p.last_ms=now;p.media_time=m.mediaTime;
              v.requestVideoFrameCallback(observe);};v.requestVideoFrameCallback(observe);return true;})()""",
            bool,
        )
        if isinstance(installed, Rejected):
            return installed
        if not installed.value:
            return Rejected(errno.EIO, "presentation observer was not installed")
        return Ok(cls(tab))

    def read(self, warmup_seconds: float) -> Result[Presentation]:
        if warmup_seconds < 0 or not math.isfinite(warmup_seconds):
            return Rejected(errno.EINVAL, "invalid presentation warmup")
        snapshot = self._tab.evaluate("window.__rkmppPresentation", _Observed)
        if isinstance(snapshot, Rejected):
            return snapshot
        observed = snapshot.value
        gaps = [
            gap.milliseconds for gap in observed.gaps if gap.elapsed >= warmup_seconds
        ]
        summary: Distribution | None = None
        availability: Literal["available", "no-callbacks", "no-steady-intervals"] = (
            "no-callbacks" if observed.callbacks == 0 else "no-steady-intervals"
        )
        if gaps:
            distribution = describe(gaps)
            if isinstance(distribution, Rejected):
                return distribution
            summary = distribution.value
            availability = "available"
        return Ok(
            Presentation(
                callbacks=observed.callbacks,
                frames=observed.frames,
                first_ms=observed.first_ms,
                last_ms=observed.last_ms,
                maximum_gap_ms=observed.maximum_gap_ms,
                media_time=observed.media_time,
                gaps=observed.gaps,
                availability=availability,
                steady_gap=summary,
            )
        )
