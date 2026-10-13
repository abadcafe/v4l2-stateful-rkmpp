"""module: test_sessions; responsibility: specify browser pipeline session lifetimes.

operations: pytest cases; entities: Secondary, PresentedExtent, Screenshot;
tests: source change, concurrent decoders and navigation cancellation/recovery.
Uses only the browser's public DOM/CDP interface and the real production bridge.
"""

import base64
import json
import time
from pathlib import Path

from pydantic import BaseModel

from tests.browser.conftest import BrowserFactory
from tests.support.outcome import accepted
from tests.support.pixels import crop_rgb, decode_png
from tests.support.player import media_properties
from tests.support.service_runtime import Running, sample


class Secondary(BaseModel):
    ended: bool
    error: str | None
    width: int
    height: int
    total: int


class PresentedExtent(BaseModel):
    width: int
    height: int


class Screenshot(BaseModel):
    data: str


def test_browser_source_change_replaces_capture_and_displays_new_extent(
    browser_factory: BrowserFactory,
    tmp_path: Path,
) -> None:
    browser = browser_factory(())
    assert accepted(
        browser.tab.evaluate(
            """(()=>{window.extents=[];const v=document.querySelector('#video');
        const observe=(now,m)=>{const last=window.extents.at(-1);
          if(!last||last.width!==m.width||last.height!==m.height)
            window.extents.push({width:m.width,height:m.height});
          v.requestVideoFrameCallback(observe);};
        v.requestVideoFrameCallback(observe);return true;})()""",
            bool,
        )
    )
    assert accepted(browser.player.load(browser.url("browser-vp9-resize.webm")))
    sizes: set[tuple[int, int]] = set()
    deadline = time.monotonic() + 15
    state = accepted(browser.player.state())
    while not state.ended:
        extent = state.width, state.height
        if extent not in sizes:
            shot = Screenshot.model_validate(
                accepted(browser.tab.call("Page.captureScreenshot", {"format": "png"}))
            )
            png = base64.b64decode(shot.data, validate=True)
            _ = (tmp_path / f"extent-{extent[0]}x{extent[1]}.png").write_bytes(png)
            image = accepted(decode_png(png))
            viewport = accepted(browser.tab.evaluate("window.innerWidth", int))
            factor = image.width / viewport
            box = state.box
            pixels = accepted(
                crop_rgb(
                    image,
                    int(box.x * factor),
                    int(box.y * factor),
                    int(box.width * factor),
                    int(box.height * factor) - 24,
                )
            )
            assert sum(pixels) / len(pixels) > 15, "new video extent is black"
            assert max(pixels) - min(pixels) > 100, "new video extent lacks content"
            sizes.add(extent)
        assert state.error is None and time.monotonic() < deadline, state
        time.sleep(0.02)
        state = accepted(browser.player.state())
    sizes.add((state.width, state.height))
    presented = accepted(browser.tab.evaluate("window.extents", list[PresentedExtent]))
    _ = (tmp_path / "source-change.json").write_text(state.model_dump_json(indent=2))
    _ = (tmp_path / "presented-extents.json").write_text(
        json.dumps([extent.model_dump() for extent in presented])
    )
    assert {(extent.width, extent.height) for extent in presented} == {
        (256, 128),
        (512, 256),
    }, (sizes, presented)
    assert (state.width, state.height, state.frames.total) == (512, 256, 48)
    assert (
        accepted(media_properties(browser.tab.events()))["kVideoDecoderName"]
        == "V4L2VideoDecoder"
    )
    _ = (tmp_path / "source-change.json").write_text(state.model_dump_json(indent=2))


def test_browser_two_hardware_videos_complete_independently(
    browser_factory: BrowserFactory,
) -> None:
    browser = browser_factory(())
    assert accepted(browser.player.load(browser.url("browser-vp9-10.webm")))
    assert accepted(browser.player.pause())
    assert accepted(
        browser.tab.evaluate(
            "(async()=>{const v=document.createElement('video');v.id='other';v.muted=true;"
            + "document.body.append(v);const ready=new Promise((ok,bad)=>{v.addEventListener('canplaythrough',ok,{once:true});"
            + "v.addEventListener('error',()=>bad(v.error.message),{once:true});});v.src="
            + json.dumps(browser.url("browser-av1-10.webm"))
            + ";await ready;await Promise.all([v.play(),document.querySelector('#video').play()]);return true;})()",
            bool,
        )
    )
    deadline = time.monotonic() + 15
    while True:
        state = accepted(browser.player.state())
        secondary = accepted(
            browser.tab.evaluate(
                "(()=>{const v=document.querySelector('#other');return {ended:v.ended,error:v.error?.message??null,"
                + "width:v.videoWidth,height:v.videoHeight,total:v.getVideoPlaybackQuality().totalVideoFrames};})()",
                Secondary,
            )
        )
        assert state.error is None and secondary.error is None
        if state.ended and secondary.ended:
            break
        assert time.monotonic() < deadline
        time.sleep(0.05)
    assert (state.frames.total, secondary.total) == (36, 36)
    assert (state.width, state.height, secondary.width, secondary.height) == (
        256,
        128,
        256,
        128,
    )
    properties = [
        event
        for event in browser.tab.events()
        if event.method == "Media.playerPropertiesChanged"
    ]
    # Each video must publish the platform decoder name, not just the most recent player.
    names: list[str] = []
    for event in properties:
        update = accepted(media_properties((event,)))
        if "kVideoDecoderName" in update:
            names.append(update["kVideoDecoderName"])
    assert len(names) >= 2 and set(names) == {"V4L2VideoDecoder"}


def test_browser_navigation_cancels_decode_releases_service_and_starts_fresh(
    browser_factory: BrowserFactory,
    bridge_service: Running,
    tmp_path: Path,
) -> None:
    browser = browser_factory(())
    existing_sessions = set(accepted(sample(bridge_service)).sessions)
    # Establish a warm baseline after MPP/Vulkan initialization. Reload
    # destroys the old document; normal navigation may retain it in BFCache.
    assert accepted(browser.player.load(browser.url("browser-vp9-10.webm")))
    deadline = time.monotonic() + 10
    while not accepted(browser.player.state()).ended:
        assert time.monotonic() < deadline
        time.sleep(0.05)
    _ = accepted(browser.tab.call("Page.reload"))
    while accepted(browser.tab.evaluate("document.readyState", str)) != "complete":
        assert time.monotonic() < deadline
        time.sleep(0.02)
    while set(accepted(sample(bridge_service)).sessions) - existing_sessions:
        assert time.monotonic() < deadline
        time.sleep(0.02)
    time.sleep(0.05)
    initial = accepted(sample(bridge_service))
    baseline = initial.fds, initial.threads, initial.sessions
    assert accepted(browser.player.load(browser.url("vp9-10-4k30-seed.webm")))
    playing = accepted(browser.player.state())
    assert playing.frames.total > 0 and not playing.ended
    active = accepted(sample(bridge_service))
    assert set(active.sessions) - set(initial.sessions), (initial, active)
    _ = accepted(browser.tab.call("Page.reload"))
    deadline = time.monotonic() + 10
    while accepted(browser.tab.evaluate("document.readyState", str)) != "complete":
        assert time.monotonic() < deadline
        time.sleep(0.02)
    while True:
        current = accepted(sample(bridge_service))
        if (current.fds, current.threads, current.sessions) == baseline:
            break
        assert time.monotonic() < deadline, (initial, current)
        time.sleep(0.02)
    assert accepted(browser.player.load(browser.url("browser-vp9-8.webm")))
    state = accepted(browser.player.state())
    while not state.ended:
        assert state.error is None and time.monotonic() < deadline
        time.sleep(0.05)
        state = accepted(browser.player.state())
    assert state.frames.total == 36
    assert (
        accepted(media_properties(browser.tab.events()))["kVideoDecoderName"]
        == "V4L2VideoDecoder"
    )
    _ = (tmp_path / "navigation-recovery.json").write_text(
        state.model_dump_json(indent=2)
    )
