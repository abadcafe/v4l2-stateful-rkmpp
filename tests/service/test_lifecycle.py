"""module: test_lifecycle; responsibility: specify same-session decoder restart.

operations: pytest cases; entities: none; tests: reset re-registers and preserves pixels.
"""

from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec


@pytest.mark.parametrize(
    ("codec", "file", "depth"),
    [
        (Codec.H264, "h264-yuv420p.h264", 8),
        (Codec.HEVC, "hevc-yuv420p.hevc", 8),
        (Codec.VP8, "vp8-yuv420p.ivf", 8),
        (Codec.VP9, "vp9-yuv420p10le.ivf", 10),
        (Codec.AV1, "av1-yuv420p10le.ivf", 10),
    ],
)
@pytest.mark.parametrize("repetitions", [2, 8])
def test_decode_same_session_reset_replaces_pools_and_replays_all_pixels(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
    depth: int,
    repetitions: int,
) -> None:
    expected = accepted(frame_hashes(artifacts / file, depth, 256, 128))
    result = accepted(play(mock_service, codec, artifacts / file, repetitions))
    assert result.eos and len(result.frames) == len(expected) * repetitions
    assert tuple(frame.digest for frame in result.frames) == expected * repetitions
    assert tuple(frame.epoch for frame in result.frames) == tuple(
        epoch for epoch in range(1, repetitions + 1) for _ in expected
    )
    assert result.layouts == ((256, 128),) * repetitions
    assert result.registrations == tuple(
        (direction, epoch)
        for epoch in range(1, repetitions + 1)
        for direction in (10, 9)
    )


@pytest.mark.parametrize(
    "codec,file,depth",
    [
        (Codec.VP8, "vp8-yuv420p.ivf", 8),
        (Codec.VP9, "vp9-yuv420p.ivf", 8),
        (Codec.VP9, "vp9-yuv420p10le.ivf", 10),
        (Codec.AV1, "av1-yuv420p.ivf", 8),
        (Codec.AV1, "av1-yuv420p10le.ivf", 10),
    ],
)
@pytest.mark.parametrize("capture_restart", [False, True])
def test_decode_drain_resume_preserves_reference_frames(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
    depth: int,
    capture_restart: bool,
) -> None:
    expected = accepted(frame_hashes(artifacts / file, depth, 256, 128))
    result = accepted(
        play(
            mock_service,
            codec,
            artifacts / file,
            drain_at=18,
            capture_restart=capture_restart,
        )
    )
    assert result.eos and len(result.frames) == 36
    assert tuple(frame.digest for frame in result.frames) == expected
    assert result.registrations == (
        ((10, 1), (9, 1), (9, 2)) if capture_restart else ((10, 1), (9, 1))
    )


@pytest.mark.parametrize(
    "codec,file",
    [
        (Codec.VP8, "vp8-yuv420p.ivf"),
        (Codec.VP9, "vp9-yuv420p.ivf"),
        (Codec.AV1, "av1-yuv420p.ivf"),
    ],
)
def test_capture_stop_aborts_drain_without_losing_codec_reference_state(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
) -> None:
    result = accepted(
        play(
            mock_service,
            codec,
            artifacts / file,
            drain_at=18,
            abort_drain=True,
            input_count=1,
        )
    )
    assert result.eos and len(result.frames) == 36
    assert result.registrations == ((10, 1), (9, 1), (9, 2))
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(artifacts / file, 8, 256, 128)
    )
