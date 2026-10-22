"""module: test_hevc_temporal; responsibility: temporal header error attribution.

operations: process cases; entities: none; tests: active-SPS layer bound,
VPS/SPS zero temporal ID, input reuse and exact IDR recovery pixels.
"""

from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec, Packet, load


@pytest.mark.parametrize("input_count", [1, 8])
@pytest.mark.parametrize(
    "nal_type", [3, 32, 33], ids=["picture-outside-sps", "vps-nonzero", "sps-nonzero"]
)
def test_temporal_header_violation_rejects_only_its_submission_and_recovers(
    mock_service: Process, artifacts: Path, input_count: int, nal_type: int
) -> None:
    source = artifacts / "hevc-yuv420p.hevc"
    packets = accepted(load(Codec.HEVC, source))
    bad = bytearray(packets[1 if nal_type == 3 else 0].data)
    original = 1 if nal_type == 3 else nal_type
    offset = bad.find(b"\x00\x00\x01" + bytes([original << 1]))
    assert offset >= 0 and bad[offset + 4] == 1
    # H.265: the real SPS declares only temporal layer0. TSA_R layer1
    # exceeds it; VPS/SPS NALs independently require TemporalId0.
    # Change only NAL header bytes and retain all entropy-coded data.
    bad[offset + 3] = nal_type << 1
    bad[offset + 4] = 2
    submissions = tuple(
        Packet(data=data, timestamp=index * 1000)
        for index, data in enumerate((packets[0].data, bytes(bad), packets[0].data))
    )
    result = accepted(
        play(
            mock_service,
            Codec.HEVC,
            source,
            input_count=input_count,
            submissions=submissions,
        )
    )
    assert result.eos and not result.errors
    assert tuple(bool(done.flags & 2) for done in result.input_completions) == (
        False,
        True,
        False,
    )
    expected = accepted(frame_hashes(source, 8, 256, 128))[0]
    assert tuple(frame.digest for frame in result.frames) == (expected, expected)
