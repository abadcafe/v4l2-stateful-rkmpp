"""module: test_picture_order; responsibility: normative presentation boundaries.

Public fixtures: FFmpeg samples V-codecs/h264/PAFF/DVD500k.mp4 (first 12
frames, Annex B remux); FATE hevc-conformance/NoOutPrior_{A,B}_Qualcomm_1.bit;
FATE h264-conformance/MR5_TANDBERG_C.264 (first 40 frames, stream copy).
operations: pytest cases; entities: none.

tests: test_presentation_matches_conformance_pixels,
test_h264_recovery_point_waits_until_clean_output,
test_maximum_dpb_pressure_reclaims_working_surfaces_across_restarts.
"""

from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec


@pytest.mark.parametrize(
    "codec,file,width,height,count",
    [
        (Codec.H264, "h264-paff.h264", 720, 576, 12),
        # Known failure on the tested RK3588 vendor/rockchip-mpp stack:
        # all 40 frames reach EOS, but presentation indices (zero-based)
        # 22, 23, 29, 30, 31, 33, 34, 38, 39 differ from the software reference.
        # MMCO 5 resets H.264 reference-picture marking and picture-order state.
        # Direct librockchip_mpp decoding, bypassing this bridge, reproduces
        # the same nine pixel mismatches in both asynchronous and no-thread
        # modes; disabling fast play does not fix them. FFmpeg software output
        # was checked against the original FATE conformance reference.
        # The earlier 18/32-buffer comparison was byte-identical. The current
        # bridge negotiates the full reference/display budget up front; no
        # temporary decode surfaces are allocated.
        # This localizes the failure to the MPP/vendor decode path, but does
        # not establish whether the library, kernel driver or hardware causes it.
        # This dependency failure remains outside the project's repair scope.
        # Keep the exact pixel assertion: do not skip, xfail, or bless bad hashes.
        (Codec.H264, "h264-mmco5.h264", 176, 144, 40),
        (Codec.HEVC, "hevc-no-prior-a.hevc", 832, 480, 40),
        (Codec.HEVC, "hevc-no-prior-b.hevc", 832, 480, 47),
    ],
)
def test_presentation_matches_conformance_pixels(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
    width: int,
    height: int,
    count: int,
) -> None:
    path = artifacts / file
    expected = accepted(frame_hashes(path, 8, width, height))
    assert len(expected) == count
    result = accepted(
        play(
            mock_service,
            codec,
            path,
        )
    )
    assert result.eos and len(result.frames) == count
    assert tuple(frame.digest for frame in result.frames) == expected


@pytest.mark.parametrize("broken_link", [False, True])
@pytest.mark.parametrize(
    "prefix",
    [b"", b"\x80\x01\x55", b"\xff\x80\x01\x55"],
    ids=["recovery-only", "reserved-128", "reserved-383"],
)
def test_h264_recovery_point_waits_until_clean_output(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
    broken_link: bool,
    prefix: bytes,
) -> None:
    # x264 testsrc2=128x96:rate=30, 48 frames, medium; keyint=12,
    # min-keyint=12:scenecut=0:intra-refresh=1:bframes=0.
    source = artifacts / "h264-recovery.h264"
    data = source.read_bytes()
    starts = [i for i in range(len(data) - 3) if data[i : i + 3] == b"\x00\x00\x01"]
    parameter_sets = bytearray()
    recovery_offset: int | None = None
    for index, start in enumerate(starts):
        end = starts[index + 1] if index + 1 < len(starts) else len(data)
        kind = data[start + 3] & 31
        if kind in (7, 8):
            parameter_sets.extend(data[start:end])
        if kind == 6 and data[start + 4] == 6:
            recovery_offset = start
            break
    assert recovery_offset is not None
    # Reserved SEI content must not hide the following recovery message.
    position = recovery_offset + 4
    data = data[:position] + prefix + data[position:]
    stream = tmp_path / "recovery.h264"
    full_reference = accepted(frame_hashes(source, 8, 128, 96))
    if broken_link:
        marked = bytearray(data)
        # First recovery payload: ue(6), exact_match_flag, broken_link_flag.
        marked[recovery_offset + 6 + len(prefix)] |= 2
        _ = stream.write_bytes(marked)
        expected = full_reference[:12] + full_reference[18:]
    else:
        _ = stream.write_bytes(parameter_sets + data[recovery_offset:])
        expected = full_reference[18:]
    result = accepted(play(mock_service, Codec.H264, stream))
    assert result.eos and len(result.frames) == len(expected)
    assert tuple(frame.digest for frame in result.frames) == expected


def test_maximum_dpb_pressure_reclaims_working_surfaces_across_restarts(
    mock_service: Process, artifacts: Path
) -> None:
    result = accepted(
        play(
            mock_service,
            Codec.H264,
            artifacts / "h264-mmco5.h264",
            repetitions=3,
            timestamps=(987654,) * 40,
            sources=(0x10000,) * 40,
        )
    )
    assert result.eos and len(result.frames) == 120
    assert (
        mock_service.log().count(
            "direct=40 quantized=0 unregistered_copy=0 held_copy=0 foreign_copy=0"
        )
        == 3
    )
    assert all(frame.timestamp == 987654000 for frame in result.frames)
    assert all(frame.flags & 0x70000 == 0x10000 for frame in result.frames)
