"""module: test_bitstream; responsibility: specify real service bytestream semantics.

operations: pytest cases; entities: none; tests: natural EOS, timestamps, crop.
"""

import subprocess
from pathlib import Path

import pytest
from rkmpp_bridge_uapi import Reply

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec, Packet, load


@pytest.mark.parametrize(
    "codec,file,marker",
    [
        (Codec.H264, "h264-yuv420p.h264", b"\x00\x00\x01\x0b"),
        (Codec.HEVC, "hevc-yuv420p.hevc", b"\x00\x00\x01\x4a\x01"),
    ],
)
def test_natural_eos_drains_without_client_stop(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    marker: bytes,
) -> None:
    stream = tmp_path / file
    _ = stream.write_bytes((artifacts / file).read_bytes() + marker)
    result = accepted(play(mock_service, codec, stream, natural_eos=True))
    assert result.eos and len(result.frames) == 36
    assert Reply.DRAINING in result.reply_order
    assert tuple(f.digest for f in result.frames) == accepted(
        frame_hashes(artifacts / file, 8, 256, 128)
    )


@pytest.mark.parametrize(
    "codec,file,filter",
    [
        (
            Codec.H264,
            "h264-yuv420p.h264",
            "h264_metadata=crop_left=16:crop_right=16:crop_top=16:crop_bottom=16",
        ),
        (
            Codec.HEVC,
            "hevc-yuv420p.hevc",
            "hevc_metadata=crop_left=16:crop_right=16:crop_top=16:crop_bottom=16",
        ),
    ],
)
def test_visible_crop_matches_software_reference(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    filter: str,
) -> None:
    stream = tmp_path / file
    made = subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-i",
            str(artifacts / file),
            "-c",
            "copy",
            "-bsf:v",
            filter,
            "-y",
            str(stream),
        ],
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert made.returncode == 0, made.stderr.decode()
    result = accepted(play(mock_service, codec, stream))
    assert result.eos and len(result.frames) == 36
    assert result.layouts == ((224, 96),)
    assert tuple(f.digest for f in result.frames) == accepted(
        frame_hashes(stream, 8, 224, 96)
    )


@pytest.mark.parametrize(
    "codec,file", [(Codec.H264, "h264-yuv420p.h264"), (Codec.HEVC, "hevc-yuv420p.hevc")]
)
def test_complete_access_units_preserve_submission_timestamps(
    mock_service: Process, artifacts: Path, codec: Codec, file: str
) -> None:
    stream = artifacts / file
    packets = accepted(load(codec, stream))
    timestamps = tuple(index * 1000 for index in range(len(packets)))
    result = accepted(
        play(mock_service, codec, stream, input_count=1, timestamps=timestamps)
    )
    assert result.eos and len(result.frames) == len(packets) == 36
    assert sorted(frame.timestamp for frame in result.frames) == [
        timestamp * 1000 for timestamp in timestamps
    ]


@pytest.mark.parametrize(
    "codec,file,marker",
    [
        (Codec.H264, "h264-yuv420p.h264", b"\x00\x00\x01\x0b"),
        (Codec.HEVC, "hevc-yuv420p.hevc", b"\x00\x00\x01\x4a\x01"),
    ],
)
def test_natural_eos_retains_following_sequence_until_resume(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    marker: bytes,
) -> None:
    stream = tmp_path / file
    _ = stream.write_bytes(((artifacts / file).read_bytes() + marker) * 2)
    # Explicit complete AUs: HEVC demuxers may attach EOB to the next AU.
    # The client owns framing; retain the terminator on its preceding AU.
    original = accepted(load(codec, artifacts / file))
    sequence = (*original[:-1], Packet(data=original[-1].data + marker, timestamp=0))
    result = accepted(
        play(
            mock_service,
            codec,
            stream,
            submissions=sequence * 2,
            natural_eos=True,
            drain_at=len(sequence),
        )
    )
    assert result.eos and len(result.frames) == 72
    expected = accepted(frame_hashes(artifacts / file, 8, 256, 128))
    assert tuple(frame.digest for frame in result.frames) == expected * 2


def test_ten_bit_cropped_output_matches_dithered_reference(
    mock_service: Process, artifacts: Path, tmp_path: Path
) -> None:
    stream = tmp_path / "crop.hevc"
    made = subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-i",
            str(artifacts / "hevc-main10.hevc"),
            "-c",
            "copy",
            "-bsf:v",
            "hevc_metadata=crop_left=16:crop_right=16:crop_top=16:crop_bottom=16",
            "-y",
            str(stream),
        ],
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert made.returncode == 0, made.stderr.decode()
    result = accepted(play(mock_service, Codec.HEVC, stream))
    assert result.layouts == ((1248, 704),) and result.eos
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(stream, 10, 1248, 704)
    )


def test_odd_visible_dimensions_preserve_all_chroma_samples(
    mock_service: Process, tmp_path: Path
) -> None:
    stream = tmp_path / "odd.ivf"
    made = subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=65x65:rate=1",
            "-frames:v",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libvpx-vp9",
            "-lossless",
            "1",
            "-y",
            str(stream),
        ],
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert made.returncode == 0, made.stderr.decode()
    result = accepted(play(mock_service, Codec.VP9, stream))
    assert result.layouts == ((65, 65),) and result.eos
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(stream, 8, 65, 65)
    )


@pytest.mark.parametrize(
    "codec,file", [(Codec.H264, "h264-yuv420p.h264"), (Codec.VP9, "vp9-yuv420p.ivf")]
)
def test_capture_stop_during_decode_preserves_input_and_reference_frames(
    mock_service: Process, artifacts: Path, codec: Codec, file: str
) -> None:
    stream = artifacts / file
    result = accepted(play(mock_service, codec, stream, restart_at_frame=5))
    assert result.eos
    assert tuple(frame.digest for frame in result.frames) == accepted(
        frame_hashes(stream, 8, 256, 128)
    )


@pytest.mark.parametrize(
    "codec,file,bsf",
    [
        (Codec.H264, "h264-yuv420p.h264", "h264_metadata"),
        (Codec.HEVC, "hevc-yuv420p.hevc", "hevc_metadata"),
    ],
)
@pytest.mark.parametrize("reuse", [False, True])
@pytest.mark.parametrize("change", ["color", "crop"])
def test_same_size_metadata_change_is_announced_at_frame_boundary(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    bsf: str,
    reuse: bool,
    change: str,
) -> None:
    filters = (
        (
            "colour_primaries=1:transfer_characteristics=1:matrix_coefficients=1",
            "colour_primaries=9:transfer_characteristics=16:matrix_coefficients=9",
        )
        if change == "color"
        else ("crop_left=16:crop_right=16", "crop_left=0:crop_right=32")
    )
    parts: list[Path] = []
    for index, options in enumerate(filters):
        part = tmp_path / f"part-{index}-{file}"
        made = subprocess.run(
            [
                "ffmpeg",
                "-v",
                "error",
                "-i",
                str(artifacts / file),
                "-c",
                "copy",
                "-bsf:v",
                f"{bsf}={options}",
                "-y",
                str(part),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        assert made.returncode == 0, made.stderr
        parts.append(part)
    stream = tmp_path / file
    _ = stream.write_bytes(b"".join(part.read_bytes() for part in parts))
    result = accepted(play(mock_service, codec, stream, reuse_capture=reuse))
    assert result.eos and len(result.frames) == 72
    assert len(result.layouts) == 2
    width = 256 if change == "color" else 224
    expected = tuple(
        digest
        for part in parts
        for digest in accepted(frame_hashes(part, 8, width, 128))
    )
    assert tuple(frame.digest for frame in result.frames) == expected
    if change == "color":
        assert all(
            (f.colorspace, f.transfer, f.ycbcr) == (3, 1, 2) for f in result.frames[:36]
        )
        assert all(
            (f.colorspace, f.transfer, f.ycbcr) == (10, 7, 6)
            for f in result.frames[36:]
        )


@pytest.mark.parametrize(
    "codec,file,marker",
    [
        (Codec.H264, "h264-yuv420p.h264", b"\x00\x00\x01\x0b"),
        (Codec.HEVC, "hevc-yuv420p.hevc", b"\x00\x00\x01\x4a\x01"),
    ],
)
def test_natural_eos_resumes_already_queued_next_sequence(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    marker: bytes,
) -> None:
    stream = tmp_path / file
    _ = stream.write_bytes(((artifacts / file).read_bytes() + marker) * 2)
    original = accepted(load(codec, artifacts / file))
    sequence = (*original[:-1], Packet(data=original[-1].data + marker, timestamp=0))
    result = accepted(
        play(
            mock_service,
            codec,
            stream,
            submissions=sequence * 2,
            natural_eos=True,
            natural_sequences=2,
        )
    )
    assert result.eos and len(result.frames) == 72
    assert result.reply_order.count(Reply.DRAINING) == 2
    assert result.reply_order.count(Reply.EOS) == 2
    assert (
        tuple(f.digest for f in result.frames)
        == accepted(frame_hashes(artifacts / file, 8, 256, 128)) * 2
    )


@pytest.mark.parametrize(
    "codec,file,marker,sets",
    [
        (Codec.H264, "h264-yuv420p.h264", b"\x00\x00\x01\x0b", (7, 8)),
        (Codec.HEVC, "hevc-yuv420p.hevc", b"\x00\x00\x01\x4a\x01", (32, 33, 34)),
    ],
)
def test_natural_eos_reuses_parameter_sets_without_retransmission(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    marker: bytes,
    sets: tuple[int, ...],
) -> None:
    original = accepted(load(codec, artifacts / file))
    continued: list[Packet] = []
    for packet in original:
        data = packet.data
        starts = [i for i in range(len(data) - 3) if data[i : i + 3] == b"\x00\x00\x01"]
        continuation = bytearray()
        for index, start in enumerate(starts):
            end = starts[index + 1] if index + 1 < len(starts) else len(data)
            kind = (
                data[start + 3] & 31
                if codec == Codec.H264
                else (data[start + 3] >> 1) & 63
            )
            if kind not in sets:
                continuation.extend(data[start:end])
        continued.append(Packet(data=bytes(continuation), timestamp=packet.timestamp))
    first = (*original[:-1], Packet(data=original[-1].data + marker, timestamp=0))
    second = (*continued[:-1], Packet(data=continued[-1].data + marker, timestamp=0))
    submissions = first + second
    stream = tmp_path / file
    _ = stream.write_bytes(b"".join(packet.data for packet in submissions))
    result = accepted(
        play(
            mock_service,
            codec,
            stream,
            submissions=submissions,
            natural_eos=True,
            natural_sequences=2,
        )
    )
    expected = accepted(frame_hashes(artifacts / file, 8, 256, 128))
    assert result.eos
    assert tuple(f.digest for f in result.frames) == expected * 2
