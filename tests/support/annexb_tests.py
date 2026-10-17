"""module: annexb_tests; responsibility: complete AU framing and termination.

operations: pytest cases; entities: none; tests: termination retention and
leading terminators assigned to their preceding access unit without byte loss.
"""

from pathlib import Path

import pytest

from tests.support.annexb import packets
from tests.support.outcome import accepted


@pytest.mark.parametrize(
    "codec,file,size,tail",
    [
        ("h264", "h264-yuv420p.h264", 4454, bytes([0, 0, 1, 10, 0, 0, 1, 11])),
        ("hevc", "hevc-yuv420p.hevc", 5496, bytes([0, 0, 1, 72, 1, 0, 0, 1, 74, 1])),
    ],
)
def test_packets_preserves_picture_and_combined_termination(
    artifacts: Path, codec: str, file: str, size: int, tail: bytes
) -> None:
    data = (artifacts / file).read_bytes()[:size] + tail
    assert accepted(packets(data, codec)) == (data,)


def test_packets_hevc_sequence_end_belongs_to_preceding_access_unit(
    artifacts: Path,
) -> None:
    data = (artifacts / "hevc-no-prior-a.hevc").read_bytes()
    framed = accepted(packets(data, "hevc"))
    assert b"".join(framed) == data
    assert len(framed) == 50
    marker = bytes([0, 0, 1, 72, 1])
    assert marker in framed[24]
    assert marker not in framed[25]
