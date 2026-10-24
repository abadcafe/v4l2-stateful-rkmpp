"""module: test_framed_boundaries; responsibility: reject missing coded partitions.

operations: pytest cases; entities: none; tests: declared VP8 first-partition
bounds and VP9 tile presence, before/after valid decoding and after reset.
"""

import struct
from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec, Packet, load


@pytest.mark.parametrize(
    "codec,file,tail",
    [
        (Codec.H264, "h264-yuv420p.h264", b"\x00\x00\x01"),
        (Codec.HEVC, "hevc-yuv420p.hevc", b"\x00\x00\x01"),
        (Codec.HEVC, "hevc-yuv420p.hevc", b"\x00\x00\x01\x40"),
    ],
)
@pytest.mark.parametrize("established", [False, True])
def test_incomplete_annexb_tail_reports_damage_and_preserves_recovery(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
    tail: bytes,
    established: bool,
) -> None:
    source = artifacts / file
    packets = accepted(load(codec, source))
    prefix = packets if established else ()
    bad = Packet(data=packets[0].data + tail, timestamp=0)
    submissions = (*prefix, bad, *packets)
    result = accepted(
        play(mock_service, codec, source, 2, input_count=1, submissions=submissions)
    )
    assert result.eos and not result.errors
    assert tuple(bool(done.flags & 2) for done in result.input_completions) == (
        tuple(index == len(prefix) for index in range(len(submissions))) * 2
    )
    expected = accepted(frame_hashes(source, 8, 256, 128))
    assert tuple(frame.digest for frame in result.frames) == expected * (
        4 if established else 2
    )


@pytest.mark.parametrize(
    "codec,file",
    [
        (Codec.VP8, "vp8-yuv420p.ivf"),
        (Codec.VP9, "vp9-yuv420p.ivf"),
        (Codec.AV1, "av1-two-tiles.ivf"),
    ],
)
@pytest.mark.parametrize("established", [False, True])
def test_missing_partition_reports_error_and_recovers_across_reset(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    established: bool,
) -> None:
    source = artifacts / file
    packets = accepted(load(codec, source))
    if codec == Codec.VP8:
        damaged = bytearray(packets[0].data)
        # Declared first partition is zero, even though the packet still has
        # enough bytes for a permissive parser to read the header from it.
        damaged[0] &= 31
        damaged[1:3] = b"\0\0"
        bad = bytes(damaged)
    elif codec == Codec.AV1:
        damaged = bytearray(packets[0].data)
        assert damaged[25] == 0
        # AV1 5.10: FRAME OBUs require an implicit full tile group.
        damaged[25] = 0xA0
        bad = bytes(damaged)
    else:
        # 18 bytes of uncompressed header + 154 bytes of compressed header,
        # with all tile data removed from the valid fixture's first picture.
        bad = packets[0].data[:172]
    prefix = tuple(packet.data for packet in packets) if established else ()
    payloads = (*prefix, bad, *(packet.data for packet in packets))
    header = bytearray(source.read_bytes()[:32])
    struct.pack_into("<I", header, 24, len(payloads))
    stream = tmp_path / file
    _ = stream.write_bytes(
        bytes(header)
        + b"".join(
            struct.pack("<IQ", len(payload), index) + payload
            for index, payload in enumerate(payloads)
        )
    )
    result = accepted(play(mock_service, codec, stream, 2, input_count=1))
    assert result.eos
    assert (
        tuple(bool(done.flags & 2) for done in result.input_completions)
        == tuple(index == len(prefix) for index in range(len(payloads))) * 2
    )
    assert not result.errors
    expected = accepted(frame_hashes(source, 8, 256, 128))
    assert tuple(frame.digest for frame in result.frames) == expected * (
        4 if established else 2
    )


@pytest.mark.parametrize(
    "codec,file,tail",
    [
        (Codec.H264, "h264-yuv420p.h264", bytes([0, 0, 1, 10, 0, 0, 1, 11])),
        (Codec.HEVC, "hevc-yuv420p.hevc", bytes([0, 0, 1, 72, 1, 0, 0, 1, 74, 1])),
    ],
)
def test_access_unit_combined_termination_delivers_picture_before_eos(
    mock_service: Process, artifacts: Path, codec: Codec, file: str, tail: bytes
) -> None:
    stream = artifacts / file
    first = accepted(load(codec, stream))[0]
    result = accepted(
        play(
            mock_service,
            codec,
            stream,
            submissions=(Packet(data=first.data + tail, timestamp=0),),
            natural_eos=True,
        )
    )
    assert result.eos and len(result.frames) == 1
    assert not result.errors
    assert all(done.flags & 2 == 0 for done in result.input_completions)
    assert result.drain_frames == (1,)
    assert result.frames[0].digest == accepted(frame_hashes(stream, 8, 256, 128))[0]
