"""module: capture; responsibility: record one hardware browser playback workload.

operations: Recording, record; entities: Recording, _Screenshot;
tests: capture_tests and direct CLI hardware validation. No pytest dependency.
"""

import base64
import binascii
import errno
import hashlib
import platform
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Literal

from pydantic import BaseModel, Field

from tests.support.browser import Browser
from tests.support.browser_launch import DECODE_FLAGS
from tests.support.cdp import Payload
from tests.support.diagnostics import Identity
from tests.support.graphics import Info, get_info
from tests.support.metadata import VideoInfo, inspect
from tests.support.outcome import Ok, Rejected, Result
from tests.support.pixels import crop_rgb, decode_png
from tests.support.player import Playback, media_properties
from tests.support.service_runtime import Running
from tests.support.video_expansion import expand
from tests.tools.performance.controls import Settings
from tests.tools.performance.options import Options, Workload
from tests.tools.performance.presentation import Monitor, Presentation
from tests.tools.performance.report import Sample, Utilization, summarize
from tests.tools.performance.telemetry import Sampler


class Recording(BaseModel):
    schema_version: Literal[3] = 3
    started_at: str
    workload: Workload
    round: int
    service: Identity
    controls: Settings
    warmup_seconds: float
    duration_limit_seconds: float | None
    kernel_release: str
    module_build_note: str | None = None
    seed_sha256: str | None = None
    source_sha256: str | None = None
    source_metadata: VideoInfo | None = None
    chromium: Payload = Field(default_factory=dict)
    graphics: Info | None = None
    flags: tuple[str, ...] = DECODE_FLAGS
    shared_desktop_pid: int | None = None
    counter_scope: str = "Chromium: owned process tree; service: entire process; desktop/GPU: shared system workload; MPP: last completed driver windows with unknown freshness, not realtime or per-session occupancy"
    outcome: Literal["running", "ended", "duration-limit", "failed", "cancelled"] = (
        "running"
    )
    failure: Rejected | None = None
    playback: Playback | None = None
    timeline: list[Playback] = Field(default_factory=list)
    samples: list[Sample] = Field(default_factory=list)
    presentation: Presentation | None = None
    properties: dict[str, str] = Field(default_factory=dict)
    utilization: Utilization | None = None
    dropped_percent: float | None = None


class _Screenshot(BaseModel):
    data: str


def record(
    workload: Workload,
    round: int,
    directory: Path,
    options: Options,
    running: Running,
    settings: Settings,
) -> Result[Recording]:
    output = Recording(
        started_at=datetime.now(UTC).isoformat(),
        workload=workload,
        round=round,
        service=running.identity,
        controls=settings,
        warmup_seconds=options.warmup_seconds,
        duration_limit_seconds=options.duration_seconds,
        kernel_release=platform.release(),
    )
    playback_source = (
        directory / ("playback" + workload.source.suffix)
        if workload.expand_frames is not None
        else workload.source
    )
    browser: Browser | None = None
    save_failure: Rejected | None = None
    try:
        directory.mkdir()
    except OSError as error:
        return Rejected(error.errno or 0, f"create measurement directory: {error}")
    try:
        setup = (
            expand(workload.source, playback_source, workload.expand_frames)
            if workload.expand_frames is not None
            else Ok(None)
        )
        if isinstance(setup, Ok):
            setup = _source(output, playback_source)
        if isinstance(setup, Rejected):
            output.failure = setup
        else:
            opened = Browser.open(directory / "browser", (playback_source,), ())
            if isinstance(opened, Rejected):
                output.failure = opened
            else:
                browser = opened.value
                result = _measure(browser, output, options, running, playback_source)
                if isinstance(result, Rejected):
                    output.failure = result
                else:
                    evidence = _image(browser, directory)
                    if isinstance(evidence, Rejected):
                        output.failure = evidence
    except KeyboardInterrupt:
        output.outcome = "cancelled"
        output.failure = Rejected(errno.ECANCELED, "measurement interrupted")
    finally:
        if browser is not None:
            closed = browser.close()
            if isinstance(closed, Rejected):
                output.failure = closed
        if workload.expand_frames is not None:
            try:
                playback_source.unlink(missing_ok=True)
            except OSError as error:
                output.failure = Rejected(
                    error.errno or 0, f"remove prepared video: {error}"
                )
        if output.failure is not None and output.outcome != "cancelled":
            output.outcome = "failed"
        try:
            _ = (directory / "report.json").write_text(output.model_dump_json(indent=2))
        except OSError as error:
            save_failure = Rejected(
                error.errno or 0, f"save measurement report: {error}"
            )
    if save_failure is not None:
        return save_failure
    if output.failure is not None:
        return Rejected(
            output.failure.code,
            output.failure.operation + f"; report: {directory / 'report.json'}",
        )
    return Ok(output)


def _source(output: Recording, playback_source: Path) -> Result[None]:
    metadata = inspect(playback_source)
    if isinstance(metadata, Rejected):
        return metadata
    output.source_metadata = metadata.value
    try:
        with output.workload.source.open("rb") as seed:
            output.seed_sha256 = hashlib.file_digest(seed, "sha256").hexdigest()
        output.source_sha256 = output.seed_sha256
        if playback_source != output.workload.source:
            with playback_source.open("rb") as source:
                output.source_sha256 = hashlib.file_digest(source, "sha256").hexdigest()
        output.module_build_note = (
            Path("/sys/module/v4l2_rkmpp_bridge/notes/.note.gnu.build-id")
            .read_bytes()
            .hex()
        )
    except OSError as error:
        return Rejected(error.errno or 0, f"record source/module identity: {error}")
    return Ok(None)


def _measure(
    browser: Browser,
    output: Recording,
    options: Options,
    running: Running,
    playback_source: Path,
) -> Result[None]:
    info = get_info(browser.control)
    if isinstance(info, Rejected):
        return info
    output.graphics = info.value
    version = browser.control.call("Browser.getVersion")
    if isinstance(version, Rejected):
        return version
    output.chromium = version.value
    output.shared_desktop_pid = options.compositor_pid
    loaded = browser.player.load(browser.url(playback_source.name))
    if isinstance(loaded, Rejected):
        return loaded
    sampler = Sampler.open(browser.pid, running, options.compositor_pid)
    if isinstance(sampler, Rejected):
        return sampler
    observer = Monitor.start(browser.tab)
    if isinstance(observer, Rejected):
        return observer
    started = time.monotonic()
    progress_time = started
    media_time = -1.0
    interval = options.interval_ms / 1000
    while True:
        state = browser.player.state()
        if isinstance(state, Rejected):
            return state
        output.playback = state.value
        output.timeline.append(state.value)
        properties = media_properties(browser.tab.events())
        if isinstance(properties, Rejected):
            return properties
        output.properties.update(properties.value)
        now = time.monotonic()
        if (
            state.value.error is not None
            or not state.value.visible
            or not browser.alive
        ):
            return Rejected(errno.EIO, "playback failed, disappeared or became hidden")
        if state.value.time > media_time:
            media_time, progress_time = state.value.time, now
        if state.value.ended:
            output.outcome = "ended"
            break
        if (
            options.duration_seconds is not None
            and now - started >= options.duration_seconds
        ):
            output.outcome = "duration-limit"
            paused = browser.player.pause()
            if isinstance(paused, Rejected):
                return paused
            break
        if now - progress_time > 10:
            return Rejected(
                errno.ETIMEDOUT, "video playback made no progress for ten seconds"
            )
        delay = (
            interval
            if options.duration_seconds is None
            else min(interval, max(0, options.duration_seconds - (now - started)))
        )
        time.sleep(delay)
        sampled = sampler.value.read()
        if isinstance(sampled, Rejected):
            return sampled
        output.samples.append(sampled.value)
    return _finish(browser, output, options, observer.value)


def _finish(
    browser: Browser, output: Recording, options: Options, observer: Monitor
) -> Result[None]:
    properties = media_properties(browser.tab.events())
    if isinstance(properties, Rejected):
        return properties
    output.properties.update(properties.value)
    if (
        output.properties.get("kVideoDecoderName") != "V4L2VideoDecoder"
        or output.properties.get("kIsPlatformVideoDecoder") != "true"
    ):
        return Rejected(
            errno.ENOTSUP, "measurement did not use hardware V4L2VideoDecoder"
        )
    state = output.playback
    if (
        state is None
        or state.frames.total <= 0
        or not 0 <= state.frames.dropped < state.frames.total
    ):
        return Rejected(
            errno.ENODATA, "measurement did not produce usable video frames"
        )
    expected = output.workload.expected
    if expected is not None and (
        (state.width, state.height) != (expected.width, expected.height)
        or output.outcome == "ended"
        and state.frames.total != expected.frames
    ):
        return Rejected(
            errno.EPROTO, "preset dimensions or completed frame count changed"
        )
    presentation = observer.read(options.warmup_seconds)
    if isinstance(presentation, Rejected):
        return presentation
    output.presentation = presentation.value
    utilization = summarize(output.samples, options.warmup_seconds)
    if isinstance(utilization, Rejected):
        return utilization
    output.utilization = utilization.value
    output.dropped_percent = 100 * state.frames.dropped / state.frames.total
    return Ok(None)


def _image(browser: Browser, directory: Path) -> Result[None]:
    shot = browser.tab.call("Page.captureScreenshot", {"format": "png"})
    if isinstance(shot, Rejected):
        return shot
    from pydantic import ValidationError

    try:
        screenshot = _Screenshot.model_validate(shot.value, strict=True)
    except ValidationError as error:
        return Rejected(errno.EPROTO, str(error))
    try:
        data = base64.b64decode(screenshot.data, validate=True)
    except binascii.Error as error:
        return Rejected(errno.EPROTO, f"invalid screenshot encoding: {error}")
    image = decode_png(data)
    if isinstance(image, Rejected):
        return image
    viewport = browser.tab.evaluate("window.innerWidth", int)
    if isinstance(viewport, Rejected):
        return viewport
    state = browser.player.state()
    if isinstance(state, Rejected):
        return state
    factor = image.value.width / viewport.value
    box = state.value.box
    pixels = crop_rgb(
        image.value,
        int(box.x * factor),
        int(box.y * factor),
        int(box.width * factor),
        int(box.height * factor) - 24,
    )
    if isinstance(pixels, Rejected):
        return pixels
    try:
        _ = (directory / "video.png").write_bytes(data)
    except OSError as error:
        return Rejected(error.errno or 0, f"save video evidence: {error}")
    if (
        not pixels.value
        or sum(pixels.value) / len(pixels.value) <= 15
        or max(pixels.value) - min(pixels.value) <= 100
    ):
        return Rejected(
            errno.EIO, "video screenshot is black or lacks expected content"
        )
    return Ok(None)
