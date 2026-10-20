"""module: tests.service.test_resources; responsibility: define the tested public contract.

operations: pytest cases; entities: Snapshot, Sample, Report; tests:
    test_service_twenty_sessions_return_resources_to_baseline

Session teardown must release real daemon FD and thread resources.
"""

import subprocess
import time
from pathlib import Path

from pydantic import BaseModel

from tests.support.diagnostics import query
from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec


class Snapshot(BaseModel):
    fds: int
    threads: int


class Sample(Snapshot):
    session: int
    frames: int


class Report(BaseModel):
    pid: int
    baseline: Snapshot
    samples: list[Sample]


def test_constrained_intra_infers_capture_budget_without_reference_surfaces(
    mock_service: Process, tmp_path: Path
) -> None:
    # x264 High Intra signals constraint_set3_flag and max_num_ref_frames=0;
    # its VUI omits bitstream restrictions. E.2.1 infers buffering=0,
    # leaving only a working output surface before the bridge's minimum.
    source = tmp_path / "intra.h264"
    encoded = subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=128x128:rate=30",
            "-frames:v",
            "3",
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-profile:v",
            "high",
            "-level:v",
            "3.0",
            "-x264-params",
            "keyint=1:threads=1",
            "-f",
            "h264",
            str(source),
        ],
        capture_output=True,
        check=False,
        timeout=15,
    )
    assert encoded.returncode == 0, encoded.stderr.decode(errors="replace")
    # Level 3 has a 16-frame DPB budget at this resolution; confusing
    # that level limit with inferred intra buffering would request 32.
    result = accepted(play(mock_service, Codec.H264, source))
    assert result.eos and not result.errors
    assert result.capture_minimums == (18,)
    assert len(result.frames) == 3
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(source, 8, 128, 128)
    )


def test_service_twenty_sessions_return_resources_to_baseline(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
) -> None:
    workloads = [
        (Codec.VP9, "vp9-yuv420p10le.ivf", 36),
        (Codec.AV1, "av1-yuv420p10le.ivf", 36),
        (Codec.H264, "h264-resize.h264", 72),
        (Codec.VP9, "vp9-10-resize.ivf", 48),
        (Codec.AV1, "av1-10-resize.ivf", 48),
    ]

    def snapshot() -> Snapshot:
        status = accepted(query(mock_service.diagnostics))
        assert not status.sessions
        return Snapshot(fds=status.fds, threads=status.threads)

    def run(workload: tuple[Codec, str, int]) -> int:
        codec, file, count = workload
        result = accepted(play(mock_service, codec, artifacts / file))
        assert result.eos and len(result.frames) == count
        return count

    for workload in workloads:
        _ = run(workload)
    time.sleep(0.2)
    baseline = snapshot()
    samples: list[Sample] = []
    try:
        for index in range(20):
            frames = run(workloads[index % len(workloads)])
            deadline = time.monotonic() + 5
            current = snapshot()
            while current != baseline and time.monotonic() < deadline:
                time.sleep(0.02)
                current = snapshot()
            samples.append(
                Sample(
                    session=index + 1,
                    frames=frames,
                    fds=current.fds,
                    threads=current.threads,
                )
            )
            assert current == baseline, (baseline, current)
        assert sum(sample.frames for sample in samples) == 960
    finally:
        _ = (tmp_path / "resources.json").write_text(
            Report(
                pid=mock_service.process.pid, baseline=baseline, samples=samples
            ).model_dump_json(indent=2)
        )
