"""module: capture_tests; responsibility: define failed measurement persistence.

operations: pytest cases; entities: none; tests: source failure and report ownership.
Successful hardware scenarios run as the standalone tool, outside pytest.
"""

import json
import os
from pathlib import Path

import pytest

from tests.support.diagnostics import Control, Identity
from tests.support.outcome import Rejected, accepted
from tests.support.service_runtime import Running
from tests.tools.capture import Recording, record
from tests.tools.controls import Settings
from tests.tools.options import Workload, parse


@pytest.mark.parametrize("expand_frames", [None, 900])
def test_record_invalid_source_saves_failure_without_browser_start(
    tmp_path: Path,
    expand_frames: int | None,
) -> None:
    source = tmp_path / "bad.webm"
    _ = source.write_bytes(b"malformed video")
    options = accepted(
        parse(("--video", str(source), "--output", str(tmp_path / "out")), tmp_path)
    )
    identity = Identity(
        pid=os.getpid(),
        uid=os.geteuid(),
        start_ticks=1,
        build_id="0123456789abcdef",
        control=Control(device=1, inode=1, raw_device=1),
    )
    running = Running(identity=identity, endpoint=tmp_path / "unused")
    settings = Settings(
        original_minimum_hz=200000000,
        requested_minimum_hz=None,
        original_interval_ms=0,
        interval_ms=1000,
    )
    output = tmp_path / "measurement"
    result = record(
        Workload(name="video", source=source, expand_frames=expand_frames),
        1,
        output,
        options,
        running,
        settings,
    )
    assert isinstance(result, Rejected)
    failed = Recording.model_validate_json((output / "report.json").read_bytes())
    assert (
        failed.outcome == "failed" and failed.failure is not None and not failed.samples
    )
    assert not (output / "browser").exists()
    assert not (output / "playback.webm").exists()
    assert source.read_bytes() == b"malformed video"
    assert (
        json.loads((output / "report.json").read_text())["workload"]["name"] == "video"
    )
