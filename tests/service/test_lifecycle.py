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
        (Codec.VP9, "vp9-yuv420p10le.ivf", 10),
        (Codec.AV1, "av1-yuv420p10le.ivf", 10),
    ],
)
def test_decode_same_session_reset_replaces_pools_and_replays_all_pixels(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
    depth: int,
) -> None:
    expected = accepted(frame_hashes(artifacts / file, depth, 256, 128))
    result = accepted(play(mock_service, codec, artifacts / file, 2))
    assert result.eos and len(result.frames) == 72
    assert tuple(frame.digest for frame in result.frames) == expected * 2
    assert tuple(frame.epoch for frame in result.frames) == (1,) * 36 + (2,) * 36
    assert result.layouts == ((256, 128), (256, 128))
    assert result.registrations == ((10, 1), (9, 1), (10, 2), (9, 2))
