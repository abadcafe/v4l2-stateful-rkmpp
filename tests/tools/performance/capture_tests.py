"""module: capture_tests; responsibility: define failed measurement persistence.

operations: pytest cases; entities: none; tests: source failure, identity I/O and report ownership.
Successful hardware scenarios run as the standalone tool, outside pytest.
"""

import errno
import hashlib
import json
import os
from pathlib import Path

import pytest

from tests.support.browser import Browser
from tests.support.diagnostics import Control, Identity
from tests.support.outcome import Rejected, accepted
from tests.support.service_runtime import Running
from tests.tools.performance.capture import Recording, record
from tests.tools.performance.controls import Settings
from tests.tools.performance.options import Workload, parse


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


def _characters_read() -> int:
    return next(
        int(line.split()[1])
        for line in Path("/proc/thread-self/io").read_text().splitlines()
        if line.startswith("rchar:")
    )


@pytest.mark.parametrize("expand_frames", [None, 120])
def test_record_source_identity_reads_each_input_once_and_preserves_digests(
    artifacts: Path,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    expand_frames: int | None,
) -> None:
    source = artifacts / "h264-4k30-seed.mp4"
    seed_digest = hashlib.sha256(source.read_bytes()).hexdigest()
    options = accepted(
        parse(("--video", str(source), "--output", str(tmp_path / "out")), tmp_path)
    )
    running = Running(
        identity=Identity(
            pid=os.getpid(),
            uid=os.geteuid(),
            start_ticks=1,
            build_id="0123456789abcdef",
            control=Control(device=1, inode=1, raw_device=1),
        ),
        endpoint=tmp_path / "unused",
    )
    settings = Settings(
        original_minimum_hz=200000000,
        requested_minimum_hz=None,
        original_interval_ms=0,
        interval_ms=1000,
    )
    output = tmp_path / "measurement"
    observed: list[tuple[int, int, str]] = []
    started = _characters_read()

    def unavailable_browser(
        directory: Path, media: tuple[Path, ...], flags: tuple[str, ...]
    ) -> Rejected:
        # Observe the setup's actual read volume before this peer reads media.
        # The thread counter excludes FFprobe/FFmpeg child reads.
        read = _characters_read() - started
        assert directory == output / "browser" and len(media) == 1 and not flags
        prepared = media[0]
        observed.append(
            (
                read,
                prepared.stat().st_size,
                hashlib.sha256(prepared.read_bytes()).hexdigest(),
            )
        )
        return Rejected(errno.EACCES, "browser unavailable")

    monkeypatch.setattr(Browser, "open", staticmethod(unavailable_browser))
    result = record(
        Workload(name="video", source=source, expand_frames=expand_frames),
        1,
        output,
        options,
        running,
        settings,
    )
    assert isinstance(result, Rejected) and result.code == errno.EACCES
    assert len(observed) == 1
    read, prepared_size, prepared_digest = observed[0]
    # Budget one pass per distinct source, plus bounded metadata/process output.
    budget = source.stat().st_size + (prepared_size if expand_frames else 0)
    assert read <= budget + 65536, (read, budget)
    report = Recording.model_validate_json((output / "report.json").read_bytes())
    assert report.seed_sha256 == seed_digest
    assert report.source_sha256 == prepared_digest
    assert (report.seed_sha256 == report.source_sha256) == (expand_frames is None)
    assert report.outcome == "failed" and report.failure is not None
    assert report.failure.code == errno.EACCES and not report.samples
    assert not (output / "playback.mp4").exists()
    assert hashlib.sha256(source.read_bytes()).hexdigest() == seed_digest
