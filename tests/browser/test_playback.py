"""module: tests.browser.test_playback; responsibility: define the tested public contract.

operations: pytest cases; entities: Screenshot; tests:
    test_browser_hardware_frames_are_visible
    test_browser_replay_resumes_hardware_decoder
    test_browser_hdr_inputs_complete_hardware_decode
    test_browser_pause_stops_clock_and_seek_resumes_hardware_decode

Actual hardware decoding must reach the composed browser window.
"""

import base64
import json
import time
from pathlib import Path

import pytest
from pydantic import BaseModel

from tests.browser.conftest import BrowserFactory
from tests.support.browser import Browser
from tests.support.graphics import get_info
from tests.support.outcome import accepted
from tests.support.pixels import crop_rgb, decode_png
from tests.support.player import media_properties
from tests.support.service_runtime import Running, sample
from tests.support.video_expansion import expand


class Screenshot(BaseModel):
    data: str


class DecodedColor(BaseModel):
    primaries: str
    transfer: str
    matrix: str
    fullRange: bool


@pytest.mark.parametrize(
    ("file", "width", "height", "frames"),
    [
        ("browser-h264.mp4", 256, 128, 36),
        ("browser-h264-bframes.mp4", 1920, 1080, 60),
        ("browser-vp8.webm", 256, 128, 36),
        ("browser-vp9-8.webm", 256, 128, 36),
        ("browser-av1-8.webm", 256, 128, 36),
        ("browser-vp9-10.webm", 256, 128, 36),
        ("browser-av1-10.webm", 256, 128, 36),
    ],
    ids=["h264", "h264-bframes", "vp8", "vp9-8", "av1-8", "vp9-10", "av1-10"],
)
def test_browser_hardware_frames_are_visible(
    browser_factory: BrowserFactory,
    tmp_path: Path,
    file: str,
    width: int,
    height: int,
    frames: int,
) -> None:
    browser = browser_factory(())
    info = accepted(get_info(browser.control))
    assert info.skia and "Mali-G610" in info.renderer
    assert accepted(browser.player.load(browser.url(file)))
    deadline = time.monotonic() + 20
    state = accepted(browser.player.state())
    while not state.ended:
        assert state.error is None and time.monotonic() < deadline, state
        time.sleep(0.1)
        state = accepted(browser.player.state())
    assert state.error is None and state.visible
    assert (state.width, state.height, state.frames.total) == (width, height, frames)
    properties = accepted(media_properties(browser.tab.events()))
    assert properties["kVideoDecoderName"] == "V4L2VideoDecoder"
    assert properties["kIsPlatformVideoDecoder"] == "true"
    shot = Screenshot.model_validate(
        accepted(browser.tab.call("Page.captureScreenshot", {"format": "png"}))
    )
    png = base64.b64decode(shot.data, validate=True)
    _ = (tmp_path / "video.png").write_bytes(png)
    viewport = accepted(browser.tab.evaluate("window.innerWidth", int))
    image = accepted(decode_png(png))
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
    assert sum(pixels) / len(pixels) > 15, "video region is black"
    assert max(pixels) - min(pixels) > 100, "video region lacks expected content"
    _ = (tmp_path / "playback.json").write_text(state.model_dump_json(indent=2))
    assert browser.alive


@pytest.mark.parametrize("file", ["browser-vp9-10.webm", "browser-av1-10.webm"])
def test_browser_replay_resumes_hardware_decoder(
    browser_factory: BrowserFactory, tmp_path: Path, file: str
) -> None:
    browser = browser_factory(())
    info = accepted(get_info(browser.control))
    assert info.skia and "Mali-G610" in info.renderer
    _ = accepted(browser.player.load(browser.url(file)))
    properties: dict[str, str] = {}
    for round in range(2):
        deadline = time.monotonic() + 20
        state = accepted(browser.player.state())
        while not state.ended:
            assert state.error is None and time.monotonic() < deadline, state
            time.sleep(0.1)
            state = accepted(browser.player.state())
        assert state.frames.total == 36 * (round + 1)
        assert 0 <= state.frames.dropped < state.frames.total
        _ = (tmp_path / f"replay-{round}.json").write_text(
            state.model_dump_json(indent=2)
        )
        properties.update(accepted(media_properties(browser.tab.events())))
        assert properties["kVideoDecoderName"] == "V4L2VideoDecoder"
        if round == 0:
            assert accepted(browser.player.replay())


@pytest.mark.parametrize(
    ("file", "frames"),
    [
        ("accept-hdr-pq-vp9.webm", 20),
        ("accept-hdr-pq-av1.webm", 60),
        ("accept-hdr-hlg-vp9.webm", 20),
        ("accept-hdr-hlg-av1.webm", 60),
    ],
)
def test_browser_hdr_inputs_complete_hardware_decode(
    browser_factory: BrowserFactory,
    tmp_path: Path,
    file: str,
    frames: int,
) -> None:
    browser = browser_factory(())
    info = accepted(get_info(browser.control))
    assert info.skia and "Mali-G610" in info.renderer
    _ = accepted(browser.player.load(browser.url(file)))
    deadline = time.monotonic() + 20
    state = accepted(browser.player.state())
    while not state.ended:
        assert state.error is None and time.monotonic() < deadline, state
        time.sleep(0.1)
        state = accepted(browser.player.state())
    properties = accepted(media_properties(browser.tab.events()))
    _ = (tmp_path / "hdr-media-properties.json").write_text(
        json.dumps(properties, indent=2)
    )
    assert properties["kVideoDecoderName"] == "V4L2VideoDecoder"
    assert properties["kIsPlatformVideoDecoder"] == "true"
    assert state.frames.total == frames and state.error is None
    assert 0 <= state.frames.dropped < frames
    color = accepted(
        browser.tab.evaluate(
            "(()=>{const f=new VideoFrame(document.querySelector('video'));"
            + "const c=f.colorSpace.toJSON();f.close();return c;})()",
            DecodedColor,
        )
    )
    assert color.primaries == "bt2020" and color.matrix == "bt2020-ncl"
    assert color.transfer == ("hlg" if "hlg" in file else "pq")
    assert not color.fullRange
    _ = (tmp_path / "hdr-decoded-color.json").write_text(
        color.model_dump_json(indent=2)
    )
    # The display's dynamic-range result is evidence, not proof of physical HDR output.
    _ = (tmp_path / "hdr-playback.json").write_text(state.model_dump_json(indent=2))


def test_browser_pause_stops_clock_and_seek_resumes_hardware_decode(
    browser_factory: BrowserFactory,
) -> None:
    browser = browser_factory(())
    info = accepted(get_info(browser.control))
    assert info.skia and "Mali-G610" in info.renderer
    _ = accepted(browser.player.load(browser.url("browser-vp9-10.webm")))
    deadline = time.monotonic() + 10
    state = accepted(browser.player.state())
    while state.time < 0.5:
        assert time.monotonic() < deadline, state
        time.sleep(0.05)
        state = accepted(browser.player.state())
    assert accepted(browser.player.pause())
    paused = accepted(browser.player.state())
    time.sleep(0.3)
    still = accepted(browser.player.state())
    assert still.paused and still.time == paused.time and not still.ended
    assert accepted(browser.player.replay())
    while not accepted(browser.player.state()).ended:
        assert time.monotonic() < deadline
        time.sleep(0.1)
    properties = accepted(media_properties(browser.tab.events()))
    assert (
        properties["kVideoDecoderName"] == "V4L2VideoDecoder"
        and properties["kIsPlatformVideoDecoder"] == "true"
    )


def test_browser_long_h264_stream_preserves_input_backpressure(
    bridge_service: Running, artifacts: Path, tmp_path: Path
) -> None:
    _ = accepted(sample(bridge_service))
    video = tmp_path / "long.mp4"
    _ = accepted(expand(artifacts / "h264-4k30-seed.mp4", video, 300))
    browser = accepted(Browser.open(tmp_path / "browser", (video,), ()))
    try:
        assert accepted(browser.player.load(browser.url(video.name)))
        deadline = time.monotonic() + 30
        state = accepted(browser.player.state())
        while not state.ended:
            assert state.error is None and time.monotonic() < deadline, state
            time.sleep(0.1)
            state = accepted(browser.player.state())
        assert (state.width, state.height, state.frames.total) == (3840, 2160, 300)
        properties = accepted(media_properties(browser.tab.events()))
        assert properties["kVideoDecoderName"] == "V4L2VideoDecoder"
    finally:
        _ = accepted(browser.close())
