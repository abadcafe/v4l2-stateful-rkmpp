"""module: test_decode_backpressure; responsibility: specify minimal input credit.

operations: pytest cases; entities: none; tests: one-buffer Annex B progress.
"""

from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec


@pytest.mark.parametrize(
    "codec,file", [(Codec.H264, "h264-yuv420p.h264"), (Codec.HEVC, "hevc-yuv420p.hevc")]
)
def test_decode_client_framed_access_units_progress_with_one_input_buffer(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
) -> None:
    expected = accepted(frame_hashes(artifacts / file, 8, 256, 128))
    result = accepted(play(mock_service, codec, artifacts / file, input_count=1))
    assert result.eos
    assert tuple(frame.digest for frame in result.frames) == expected
