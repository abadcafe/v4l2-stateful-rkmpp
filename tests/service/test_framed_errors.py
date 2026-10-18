"""module: test_framed_errors; responsibility: observe framed codec damage.

operations: pytest cases; entities: none; tests: framing/header damage,
initial/established stream, input reuse, exact recovery and EOS.
The error oracle is a protocol completion, never a stderr message.
"""

import struct
from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec, load


@pytest.mark.parametrize(
    "codec,file,bad",
    [
        pytest.param(Codec.VP8, "vp8-yuv420p.ivf", b"\xff" * 16, id="vp8-envelope"),
        pytest.param(Codec.VP9, "vp9-yuv420p.ivf", b"\xff" * 16, id="vp9-envelope"),
        pytest.param(Codec.AV1, "av1-yuv420p.ivf", b"\xff" * 16, id="av1-envelope"),
        pytest.param(
            Codec.VP8,
            "vp8-yuv420p.ivf",
            bytes.fromhex("1000009d012a00010000"),
            id="vp8-header",
        ),
        pytest.param(
            Codec.VP9, "vp9-yuv420p.ivf", bytes.fromhex("82"), id="vp9-header"
        ),
        pytest.param(
            Codec.AV1, "av1-yuv420p.ivf", bytes.fromhex("3201ff"), id="av1-header"
        ),
        pytest.param(
            Codec.AV1,
            "av1-yuv420p.ivf",
            # Valid sequence and standalone keyframe header, missing tile group.
            bytes.fromhex(
                "0a0b000000f9dbfff8842a02801a1b10021ac56360bdfc1fd807fc021001120000000000041041000823"
            ),
            id="av1-missing-tiles",
        ),
    ],
)
@pytest.mark.parametrize("established", [False, True])
@pytest.mark.parametrize("repetitions", [1, 2])
def test_damaged_packet_reports_only_its_input_and_preserves_recovery_pixels(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    bad: bytes,
    established: bool,
    repetitions: int,
) -> None:
    source = artifacts / file
    packets = accepted(load(codec, source))
    prefix = tuple(p.data for p in packets) if established else ()
    payloads = (*prefix, bad, *(p.data for p in packets))
    ivf = bytearray(source.read_bytes()[:32])
    struct.pack_into("<I", ivf, 24, len(payloads))
    stream = tmp_path / file
    _ = stream.write_bytes(
        bytes(ivf)
        + b"".join(
            struct.pack("<IQ", len(payload), index) + payload
            for index, payload in enumerate(payloads)
        )
    )
    result = accepted(play(mock_service, codec, stream, repetitions, input_count=1))
    assert result.eos
    assert len(result.input_completions) == len(payloads) * repetitions
    assert (
        tuple(bool(done.flags & 2) for done in result.input_completions)
        == tuple(index == len(prefix) for index in range(len(payloads))) * repetitions
    )
    assert not result.errors
    expected = accepted(frame_hashes(source, 8, 256, 128))
    assert (
        tuple(frame.digest for frame in result.frames)
        == expected * (2 if established else 1) * repetitions
    )


@pytest.mark.parametrize(
    "bad", [b"\x7a\x00", b"\xff"], ids=["metadata-overflow", "bad-obu"]
)
def test_av1_metadata_rejection_recovers_on_first_valid_keyframe(
    mock_service: Process, artifacts: Path, bad: bytes
) -> None:
    from tests.support.stream import Packet

    source = artifacts / "av1-yuv420p.ivf"
    packets = accepted(load(Codec.AV1, source))
    # Two complete 8 MiB padding OBUs fill the retained metadata budget.
    # Reject only the following submission, then decode the FIRST subsequent
    # keyframe and its dependent pictures. No decoder reset or second keyframe
    # may be required to discard the rejected metadata transaction.
    padding = b"\x7a\xfb\xff\xff\x03" + bytes(8 * 1024 * 1024 - 5)
    prefix = tuple(
        Packet(data=data, timestamp=index)
        for index, data in enumerate((padding, padding, bad))
    )
    submissions = prefix + packets
    result = accepted(
        play(mock_service, Codec.AV1, source, input_count=1, submissions=submissions)
    )
    assert result.eos and not result.errors
    assert tuple(bool(done.flags & 2) for done in result.input_completions) == (
        False,
        False,
        True,
        *(False for _ in packets),
    )
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(source, 8, 256, 128)
    )
