"""module: test_output_semantics; responsibility: normative output eligibility.

Fixtures: x264 medium, testsrc2 128x96/256x128, three frames, bframes=2,
b-adapt=0, scenecut=0; second IDR no_output_of_prior_pics_flag=1.
HEVC: x265 testsrc2 128x96, four frames, bframes=0; PPS output flag enabled,
second picture pic_output_flag=0, slice byte alignment regenerated.
operations: pytest cases; entities: none; tests: discard, hidden pictures, backpressure.
"""

import subprocess
from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec


def test_source_change_discards_pending_old_pictures(
    mock_service: Process, artifacts: Path
) -> None:
    result = accepted(
        play(mock_service, Codec.H264, artifacts / "h264-discard-change.h264")
    )
    expected = accepted(frame_hashes(artifacts / "h264-discard-old.h264", 8, 128, 96))[
        :1
    ]
    expected += accepted(frame_hashes(artifacts / "h264-discard-new.h264", 8, 256, 128))
    assert result.eos
    assert tuple(frame.digest for frame in result.frames) == expected


def test_hevc_hidden_reference_is_decoded_without_display(
    mock_service: Process, artifacts: Path
) -> None:
    path = artifacts / "hevc-hidden-picture.hevc"
    expected = accepted(frame_hashes(path, 8, 128, 96))
    assert len(expected) == 3
    result = accepted(play(mock_service, Codec.HEVC, path))
    assert result.eos
    assert tuple(frame.digest for frame in result.frames) == expected


@pytest.mark.parametrize("input_count", [1, 8])
def test_many_client_framed_pictures_are_independent_of_queue_depth(
    mock_service: Process, tmp_path: Path, input_count: int
) -> None:
    path = tmp_path / "many.h264"
    encoded = subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=black:size=64x64:rate=30",
            "-frames:v",
            "4100",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-x264-params",
            "bframes=0:keyint=30:scenecut=0",
            "-y",
            str(path),
        ],
        capture_output=True,
        check=False,
        timeout=30,
    )
    assert encoded.returncode == 0, encoded.stderr
    expected = accepted(frame_hashes(path, 8, 64, 64))
    result = accepted(play(mock_service, Codec.H264, path, input_count=input_count))
    assert result.eos and len(result.frames) == 4100
    assert tuple(frame.digest for frame in result.frames) == expected
