"""module: player; responsibility: observe actual browser video playback.

operations: Player load/state/replay/pause, media_properties; entities: Player,
Playback, Quality, Box, Property, Properties; tests: player_tests, browser cases.
"""

import errno
import json
from collections.abc import Sequence

from pydantic import BaseModel, ValidationError

from tests.support.cdp import Cdp, Notification
from tests.support.outcome import Ok, Rejected, Result


class Quality(BaseModel):
    total: int
    dropped: int


class Box(BaseModel):
    x: float
    y: float
    width: float
    height: float


class Playback(BaseModel):
    time: float
    ended: bool
    paused: bool
    error: str | None
    width: int
    height: int
    frames: Quality
    hdr: bool
    visible: bool
    focused: bool
    box: Box


class Property(BaseModel):
    name: str
    value: str


class Properties(BaseModel):
    properties: list[Property]


def media_properties(events: Sequence[Notification]) -> Result[dict[str, str]]:
    properties: dict[str, str] = {}
    for event in events:
        if event.method != "Media.playerPropertiesChanged":
            continue
        try:
            update = Properties.model_validate(event.params, strict=True)
        except ValidationError as error:
            return Rejected(errno.EPROTO, str(error))
        for item in update.properties:
            properties[item.name] = item.value
    return Ok(properties)


class Player:
    def __init__(self, tab: Cdp) -> None:
        self._tab: Cdp = tab

    def load(self, url: str) -> Result[bool]:
        return self._tab.evaluate(
            "(async()=>{const v=document.querySelector('video');v.preload='auto';const ready=new Promise((resolve,reject)=>{v.addEventListener('canplaythrough',resolve,{once:true});v.addEventListener('error',()=>reject(v.error.message),{once:true});});v.src="
            + json.dumps(url)
            + ";await ready;await v.play();return true;})()",
            bool,
        )

    def replay(self) -> Result[bool]:
        return self._tab.evaluate(
            "(async()=>{const v=document.querySelector('video');v.pause();const sought=new Promise(resolve=>v.addEventListener('seeked',resolve,{once:true}));v.currentTime=0;await sought;await v.play();return true;})()",
            bool,
        )

    def pause(self) -> Result[bool]:
        return self._tab.evaluate(
            "(()=>{document.querySelector('video').pause();return true;})()", bool
        )

    def state(self) -> Result[Playback]:
        return self._tab.evaluate(
            """(()=>{const v=document.querySelector('video'),q=v.getVideoPlaybackQuality(),b=v.getBoundingClientRect();return {time:v.currentTime,ended:v.ended,paused:v.paused,error:v.error?.message??null,width:v.videoWidth,height:v.videoHeight,frames:{total:q.totalVideoFrames,dropped:q.droppedVideoFrames},hdr:matchMedia('(dynamic-range: high)').matches,visible:document.visibilityState==='visible',focused:document.hasFocus(),box:{x:b.x,y:b.y,width:b.width,height:b.height}};})()""",
            Playback,
        )
