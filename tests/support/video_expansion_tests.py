"""module: video_expansion_tests; responsibility: specify lossless measurement expansion.

operations: pytest cases; entities: probe models; tests: packets, timing, errors.
"""

import subprocess
from decimal import Decimal
from pathlib import Path

import pytest
from pydantic import BaseModel

from tests.support.metadata import inspect
from tests.support.outcome import Rejected, accepted
from tests.support.video_expansion import expand


class Packet(BaseModel):
    data_hash: str
    flags: str
    pts_time: str
    dts_time: str
    duration_time: str


class Probe(BaseModel):
    packets: list[Packet]


def probe(path: Path) -> Probe:
    result = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_packets",
            "-show_data_hash",
            "sha256",
            "-of",
            "json",
            str(path),
        ],
        capture_output=True,
        check=True,
    )
    return Probe.model_validate_json(result.stdout)


@pytest.mark.parametrize(
    "name,frames",
    [
        ("h264-4k30-seed.mp4", 60),
        ("vp9-10-4k30-seed.webm", 36),
        ("av1-10-4k30-seed.webm", 12),
    ],
)
def test_expand_preserves_all_packets_metadata_and_continuous_timestamps(
    artifacts: Path,
    tmp_path: Path,
    name: str,
    frames: int,
) -> None:
    source = artifacts / name
    destination = tmp_path / name
    original = probe(source).packets
    assert len(original) == frames
    accepted(expand(source, destination, 900))
    expanded = probe(destination).packets
    assert len(expanded) == 900
    assert accepted(inspect(source)) == accepted(inspect(destination))
    for index, packet in enumerate(expanded):
        seed = original[index % frames]
        assert (packet.data_hash, packet.flags, packet.duration_time) == (
            seed.data_hash,
            seed.flags,
            seed.duration_time,
        )
        offset = Decimal(index // frames * frames) / 30
        assert Decimal(packet.pts_time) == Decimal(seed.pts_time) + offset
        assert Decimal(packet.dts_time) == Decimal(seed.dts_time) + offset
    assert probe(source).packets == original


def test_expand_rejects_missing_source(tmp_path: Path) -> None:
    assert isinstance(
        expand(tmp_path / "missing.webm", tmp_path / "out.webm", 900), Rejected
    )


def test_expand_rejects_malformed_video(tmp_path: Path) -> None:
    source = tmp_path / "bad.webm"
    _ = source.write_bytes(b"not video")
    assert isinstance(expand(source, tmp_path / "out.webm", 900), Rejected)


def test_expand_preserves_existing_destination(artifacts: Path, tmp_path: Path) -> None:
    destination = tmp_path / "out.mp4"
    _ = destination.write_bytes(b"owned elsewhere")
    assert isinstance(
        expand(artifacts / "h264-4k30-seed.mp4", destination, 900), Rejected
    )
    assert destination.read_bytes() == b"owned elsewhere"


def test_expand_rejects_invalid_frame_count(artifacts: Path, tmp_path: Path) -> None:
    destination = tmp_path / "out.mp4"
    assert isinstance(
        expand(artifacts / "h264-4k30-seed.mp4", destination, 0), Rejected
    )
    assert not destination.exists()
