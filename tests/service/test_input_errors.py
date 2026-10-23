"""module: test_input_errors; responsibility: submission-local error attribution.

operations: process cases; entities: none; tests: immediate input errors,
slot reuse, drain and keyframe recovery under the client-framed contract.
"""

from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec, Packet, load


def test_h264_leading_dependent_picture_waits_for_resume_without_input_error(
    mock_service: Process, artifacts: Path
) -> None:
    source = artifacts / "h264-two-slices-mmco.h264"
    packets = accepted(load(Codec.H264, source))
    first = packets[0].data
    picture = first.find(b"\x00\x00\x01\x65")
    assert picture > 0
    # Parameter sets are available, but the earlier pictures targeted by
    # this dependent AU's MMCO were not decoded in this incarnation.
    inputs = (Packet(data=first[:picture], timestamp=0), packets[5]) + packets
    result = accepted(play(mock_service, Codec.H264, source, submissions=inputs))
    assert result.eos and not result.errors
    assert all(done.flags & 2 == 0 for done in result.input_completions)
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(source, 8, 128, 128)
    )


@pytest.mark.parametrize("input_count", [1, 8])
def test_semantic_rejection_does_not_configure_the_following_submission(
    mock_service: Process, artifacts: Path, input_count: int
) -> None:
    source = artifacts / "h264-yuv420p.h264"
    good = accepted(load(Codec.H264, source))
    first = good[0].data
    picture = first.find(b"\x00\x00\x01\x65")
    assert picture > 0
    # SPS/PPS are valid, but recovery_point has a truncated Exp-Golomb value.
    # Rejecting its semantics must also roll back those new parameter sets.
    damaged = first[:picture] + bytes.fromhex("0000010606010080") + first[picture:]
    inputs = (
        Packet(data=damaged, timestamp=0),
        Packet(data=first[picture:], timestamp=1000),
    ) + good
    result = accepted(
        play(
            mock_service,
            Codec.H264,
            source,
            input_count=input_count,
            submissions=inputs,
        )
    )
    assert result.eos and not result.errors
    assert [done.flags & 2 for done in result.input_completions] == [2, 2] + [0] * len(
        good
    )
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(source, 8, 256, 128)
    )


def test_h264_invalid_sei_escape_rejects_only_its_submission(
    mock_service: Process, artifacts: Path
) -> None:
    source = artifacts / "h264-yuv420p.h264"
    good = accepted(load(Codec.H264, source))
    # H.264 7.4.1 forbids 00 00 03 followed by a byte above 03.
    # Reserved SEI payload types still have to obey NAL byte syntax.
    damaged = Packet(data=bytes.fromhex("0000010680030000030480"), timestamp=0)
    result = accepted(
        play(
            mock_service,
            Codec.H264,
            source,
            input_count=1,
            submissions=(damaged,) + good,
        )
    )
    assert result.eos and not result.errors
    assert [done.flags & 2 for done in result.input_completions] == [2] + [0] * len(
        good
    )
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(source, 8, 256, 128)
    )


@pytest.mark.parametrize(
    "codec,damaged,file",
    [
        (Codec.H264, bytes.fromhex("00000165b8"), "h264-yuv420p.h264"),
        (Codec.HEVC, bytes.fromhex("0000012601e0"), "hevc-yuv420p.hevc"),
    ],
)
@pytest.mark.parametrize("input_count", [1, 8])
def test_damaged_submission_does_not_poison_reused_input(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    damaged: bytes,
    file: str,
    input_count: int,
) -> None:
    path = artifacts / file
    good = accepted(load(codec, path))
    inputs = (
        Packet(data=damaged, timestamp=0),
        Packet(data=damaged, timestamp=1000),
    ) + tuple(Packet(data=p.data, timestamp=(i + 2) * 1000) for i, p in enumerate(good))
    result = accepted(
        play(mock_service, codec, path, input_count=input_count, submissions=inputs)
    )
    assert result.eos and not result.errors
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(path, 8, 256, 128)
    )
    assert len(result.input_completions) == len(inputs)
    assert [completion.flags & 2 for completion in result.input_completions] == [
        2,
        2,
    ] + [0] * len(good)


@pytest.mark.parametrize("established", [False, True])
@pytest.mark.parametrize("input_count", [1, 8])
@pytest.mark.parametrize("level", [10, 255], ids=["understated", "unknown"])
def test_h264_advertised_level_preserves_bounded_decode_requirements(
    mock_service: Process,
    artifacts: Path,
    established: bool,
    input_count: int,
    level: int,
) -> None:
    source = artifacts / "h264-yuv420p.h264"
    good = accepted(load(Codec.H264, source))
    adjusted = bytearray(good[0].data)
    sps = adjusted.find(b"\x00\x00\x01\x67")
    assert sps >= 0 and adjusted[sps + 6] == 11
    # Keep the original four reference frames and VUI buffering=4,
    # but signal level 1, whose 396 macroblocks describe only three
    # 256x128 buffers. Level is a capability declaration: the actual
    # four-buffer requirement fits the bridge and must still decode.
    # An unknown declaration likewise must not override actual bounds.
    # Known external failure at 255: MPP consumes this IDR without
    # publishing its image. The direct Decoder regression reproduces
    # the missing output without bridge parsing or presentation state.
    # Keep exact pixels and successful completions as the requirement.
    adjusted[sps + 6] = level
    inputs = (
        ((good[0],) if established else ())
        + (Packet(data=bytes(adjusted), timestamp=0),)
        + good
    )
    result = accepted(
        play(
            mock_service,
            Codec.H264,
            source,
            input_count=input_count,
            submissions=inputs,
        )
    )
    expected = accepted(frame_hashes(source, 8, 256, 128))
    assert result.eos and not result.errors
    assert tuple(frame.digest for frame in result.frames) == (
        (expected[:1] if established else ()) + expected[:1] + expected
    )
    assert tuple(bool(done.flags & 2) for done in result.input_completions) == (
        ((False,) if established else ()) + (False,) * (1 + len(good))
    )


@pytest.mark.parametrize(
    "codec,damaged",
    [
        (Codec.H264, bytes.fromhex("00000165b8")),
        (Codec.HEVC, bytes.fromhex("0000012601e0")),
    ],
)
@pytest.mark.parametrize("natural", [False, True])
def test_damaged_final_submission_completes_before_drain(
    mock_service: Process,
    tmp_path: Path,
    codec: Codec,
    damaged: bytes,
    natural: bool,
) -> None:
    marker = bytes.fromhex("0000010b" if codec == Codec.H264 else "0000014a01")
    inputs = (Packet(data=damaged, timestamp=1000),) + (
        (Packet(data=marker, timestamp=2000),) if natural else ()
    )
    result = accepted(
        play(
            mock_service,
            codec,
            tmp_path / "unused",
            submissions=inputs,
            input_count=1,
            natural_eos=natural,
        )
    )
    assert result.eos and not result.frames and not result.errors
    assert len(result.input_completions) == len(inputs)
    assert result.input_completions[0].flags & 2
    assert all(completion.flags & 2 == 0 for completion in result.input_completions[1:])


@pytest.mark.parametrize("established", [False, True])
@pytest.mark.parametrize("input_count", [1, 8])
def test_hevc_conflicting_slice_headers_reject_only_damaged_submission(
    mock_service: Process,
    artifacts: Path,
    established: bool,
    input_count: int,
) -> None:
    source = artifacts / "hevc-two-slices.hevc"
    good = source.read_bytes()
    # H.265 7.4.7.1: no_output_of_prior_pics_flag must agree across
    # both IDR slices. Change only that bit in the second slice.
    assert good[5716:5722] == bytes.fromhex("000001280130")
    bad = bytearray(good)
    bad[5721] ^= 0x40
    payloads = ((good,) if established else ()) + (bytes(bad), good)
    result = accepted(
        play(
            mock_service,
            Codec.HEVC,
            source,
            input_count=input_count,
            submissions=tuple(
                Packet(data=data, timestamp=i * 1000) for i, data in enumerate(payloads)
            ),
        )
    )
    assert result.eos and not result.errors
    assert tuple(bool(done.flags & 2) for done in result.input_completions) == (
        ((False,) if established else ()) + (True, False)
    )
    expected = accepted(frame_hashes(source, 8, 256, 256))
    assert tuple(frame.digest for frame in result.frames) == expected * (
        2 if established else 1
    )


@pytest.mark.parametrize("established", [False, True])
@pytest.mark.parametrize("input_count", [1, 8])
@pytest.mark.parametrize("mask", [2, 1], ids=["no-prior-output", "long-term-reference"])
def test_h264_conflicting_idr_marking_rejects_only_damaged_submission(
    mock_service: Process,
    artifacts: Path,
    established: bool,
    input_count: int,
    mask: int,
) -> None:
    source = artifacts / "h264-two-slices.h264"
    good = source.read_bytes()
    # H.264 7.4.3.3: both slices of the primary picture must carry
    # identical dec_ref_pic_marking(). Flip only bit 38 or 39 of the
    # second NAL; all entropy-coded pixel data remains intact.
    assert good[3146] == 0x65
    bad = bytearray(good)
    bad[3150] ^= mask
    payloads = ((good,) if established else ()) + (bytes(bad), good)
    result = accepted(
        play(
            mock_service,
            Codec.H264,
            source,
            input_count=input_count,
            submissions=tuple(
                Packet(data=data, timestamp=i * 1000) for i, data in enumerate(payloads)
            ),
        )
    )
    assert result.eos and not result.errors
    assert tuple(bool(done.flags & 2) for done in result.input_completions) == (
        ((False,) if established else ()) + (True, False)
    )
    expected = accepted(frame_hashes(source, 8, 128, 128))
    assert tuple(frame.digest for frame in result.frames) == expected * (
        2 if established else 1
    )


@pytest.mark.parametrize("input_count", [1, 8])
def test_h264_conflicting_mmco_operand_is_submission_local(
    mock_service: Process, artifacts: Path, input_count: int
) -> None:
    source = artifacts / "h264-two-slices-mmco.h264"
    packets = accepted(load(Codec.H264, source))
    assert len(packets) == 8
    assert packets[5].data == source.read_bytes()[10155:11415]
    bad = bytearray(packets[5].data)
    # Change only the second slice's MMCO1 operand, 3 -> 4 (bit 47).
    bad[10633 - 10155] ^= 1
    inputs = tuple(
        Packet(data=data, timestamp=i * 1000)
        for i, data in enumerate((packets[0].data, bytes(bad), packets[0].data))
    )
    result = accepted(
        play(
            mock_service,
            Codec.H264,
            source,
            input_count=input_count,
            submissions=inputs,
        )
    )
    assert result.eos and not result.errors
    assert tuple(bool(done.flags & 2) for done in result.input_completions) == (
        False,
        True,
        False,
    )
    expected = accepted(frame_hashes(source, 8, 128, 128))[0]
    assert tuple(frame.digest for frame in result.frames) == (expected, expected)


def test_h264_valid_multi_slice_mmco_preserves_all_pixels(
    mock_service: Process, artifacts: Path
) -> None:
    source = artifacts / "h264-two-slices-mmco.h264"
    result = accepted(play(mock_service, Codec.H264, source, input_count=1))
    assert result.eos and not result.errors
    assert len(result.input_completions) == 8
    assert all(done.flags & 2 == 0 for done in result.input_completions)
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(source, 8, 128, 128)
    )


@pytest.mark.parametrize("input_count", [1, 8])
def test_hevc_missing_current_reference_is_rejected_before_decode_and_recovers(
    mock_service: Process, artifacts: Path, input_count: int
) -> None:
    source = artifacts / "hevc-yuv420p.hevc"
    packets = accepted(load(Codec.HEVC, source))
    # POC1 uses POC2. Omit POC2, then resume at an independently framed IDR.
    submissions = tuple(
        Packet(data=data, timestamp=index * 1000)
        for index, data in enumerate(
            (packets[0].data, packets[2].data, packets[0].data)
        )
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
