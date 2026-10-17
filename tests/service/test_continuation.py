"""module: test_continuation; responsibility: specify reference-preserving decode.

operations: pytest cases; entities: none; tests: ordinary drain, interlaced fields.
"""

import subprocess
from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes, frame_positions
from tests.support.stream import Codec, load


@pytest.mark.parametrize(
    "codec,file,depth",
    [
        (Codec.H264, "h264-yuv420p.h264", 8),
        (Codec.HEVC, "hevc-yuv420p.hevc", 8),
        (Codec.VP8, "vp8-yuv420p.ivf", 8),
        (Codec.VP9, "vp9-yuv420p.ivf", 8),
        (Codec.VP9, "vp9-yuv420p10le.ivf", 10),
        (Codec.AV1, "av1-yuv420p.ivf", 8),
        (Codec.AV1, "av1-yuv420p10le.ivf", 10),
    ],
)
@pytest.mark.parametrize("boundary", [1, 3, 17])
def test_stop_start_preserves_framed_codec_references(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
    depth: int,
    boundary: int,
) -> None:
    stream = artifacts / file
    result = accepted(
        play(mock_service, codec, stream, input_count=1, drain_at=boundary)
    )
    assert result.eos
    expected = accepted(frame_hashes(stream, depth, 256, 128))
    before_stop = boundary
    if codec in (Codec.H264, Codec.HEVC):
        # STOP must publish every pending picture before LAST. If it falls
        # between a future reference and its B pictures, those later inputs
        # necessarily display after that reference upon START. Verify the
        # normative order independently within each drain segment, not the
        # uninterrupted stream's global display order.
        packets = accepted(load(codec, stream))
        offset = sum(len(packet.data) for packet in packets[:boundary])
        positions = accepted(frame_positions(stream))
        assert len(positions) == len(expected)
        before_stop = sum(pos < offset for pos in positions)
        frames = tuple(zip(positions, expected, strict=True))
        expected = tuple(digest for pos, digest in frames if pos < offset) + tuple(
            digest for pos, digest in frames if pos >= offset
        )
    assert result.drain_frames == (before_stop, len(expected))
    assert tuple(frame.digest for frame in result.frames) == expected


@pytest.mark.parametrize("top", [True, False])
def test_interlaced_frames_preserve_pixels_and_field_order(
    mock_service: Process, tmp_path: Path, top: bool
) -> None:
    stream = tmp_path / "interlaced.h264"
    encoded = subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=256x128:rate=30",
            "-frames:v",
            "12",
            "-c:v",
            "libx264",
            "-flags",
            "+ilme+ildct",
            "-x264-params",
            "tff=1" if top else "bff=1",
            "-y",
            str(stream),
        ],
        capture_output=True,
        check=False,
        timeout=30,
    )
    assert encoded.returncode == 0, encoded.stderr
    result = accepted(play(mock_service, Codec.H264, stream))
    assert result.eos and len(result.frames) == 12
    assert all(frame.field == (1 if top else 2) for frame in result.frames)
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(stream, 8, 256, 128)
    )


@pytest.mark.parametrize("depth", [8, 10])
@pytest.mark.parametrize("count", [1, 3, 5, 6])
def test_stop_delivers_every_av1_display_without_future_input(
    mock_service: Process, artifacts: Path, depth: int, count: int
) -> None:
    stream = artifacts / ("av1-yuv420p.ivf" if depth == 8 else "av1-yuv420p10le.ivf")
    packets = accepted(load(Codec.AV1, stream))
    result = accepted(
        play(
            mock_service, Codec.AV1, stream, submissions=packets[:count], input_count=1
        )
    )
    assert result.eos
    assert (
        tuple(frame.digest for frame in result.frames)
        == accepted(frame_hashes(stream, depth, 256, 128))[:count]
    )
