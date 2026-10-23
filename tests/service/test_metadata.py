"""module: test_metadata; responsibility: specify decoded timestamp provenance.

operations: pytest cases; entities: none; tests: origin flags survive MPP and conversion.
"""

from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec, Packet, load


@pytest.mark.parametrize(
    "codec,file",
    [
        (Codec.H264, "h264-yuv420p.h264"),
        (Codec.HEVC, "hevc-yuv420p.hevc"),
        (Codec.HEVC, "hevc-main10.hevc"),
        (Codec.VP8, "vp8-yuv420p.ivf"),
        (Codec.VP9, "vp9-yuv420p.ivf"),
        (Codec.VP9, "vp9-yuv420p10le.ivf"),
        (Codec.AV1, "av1-yuv420p.ivf"),
        (Codec.AV1, "av1-yuv420p10le.ivf"),
    ],
)
def test_decode_preserves_timestamp_source(
    mock_service: Process, artifacts: Path, codec: Codec, file: str
) -> None:
    packets = accepted(load(codec, artifacts / file))
    sources = tuple(0x10000 if index % 2 == 0 else 0 for index in range(len(packets)))
    timestamps = tuple(index * 1000 for index in range(len(packets)))
    result = accepted(
        play(
            mock_service,
            codec,
            artifacts / file,
            sources=sources,
            timestamps=timestamps,
        )
    )
    assert result.eos and result.frames
    expected = {
        timestamp * 1000: source
        for timestamp, source in zip(timestamps, sources, strict=True)
    }
    assert all(
        frame.flags & 0x70000 == expected[frame.timestamp] for frame in result.frames
    )


@pytest.mark.parametrize(
    "codec,file,prefix",
    [
        (Codec.H264, "h264-yuv420p.h264", 0),
        (Codec.HEVC, "hevc-yuv420p.hevc", 0),
        (Codec.AV1, "av1-yuv420p.ivf", 15),
    ],
)
@pytest.mark.parametrize("repetitions", [1, 2])
def test_separate_configuration_uses_picture_timestamp_and_preserves_pixels(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
    prefix: int,
    repetitions: int,
) -> None:
    path = artifacts / file
    packets = accepted(load(codec, path))
    first = packets[0].data
    if not prefix:
        prefix = next(
            index
            for index in range(len(first) - 3)
            if first[index : index + 3] == b"\x00\x00\x01"
            and (
                first[index + 3] & 31 == 5
                if codec == Codec.H264
                else (first[index + 3] >> 1) & 63 <= 31
            )
        )
    submissions = (
        Packet(data=first[:prefix], timestamp=111),
        Packet(data=first[prefix:], timestamp=222),
    ) + tuple(Packet(data=packet.data, timestamp=999) for packet in packets[1:])
    result = accepted(
        play(
            mock_service,
            codec,
            path,
            repetitions=repetitions,
            input_count=1,
            submissions=submissions,
            sources=(0x10000,) + (0,) * len(packets),
        )
    )
    assert result.eos and not result.errors
    assert tuple(frame.digest for frame in result.frames) == (
        accepted(frame_hashes(path, 8, 256, 128)) * repetitions
    )
    assert result.frames[0].timestamp == 222000
    assert all(frame.flags & 0x70000 == 0 for frame in result.frames)
    assert len(result.input_completions) == len(submissions) * repetitions
    assert all(completion.flags == 0 for completion in result.input_completions)
