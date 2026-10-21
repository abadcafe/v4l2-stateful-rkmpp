"""module: test_decode; responsibility: specify real process codec output.

operations: pytest cases; entities: none; tests: decode pixel/timestamp/color/EOS.
The kernel endpoint is simulated; production MPP/Vulkan execution is real.
"""

from pathlib import Path

import pytest
from rkmpp_bridge_uapi import Reply

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec, load


@pytest.mark.parametrize(
    ("codec", "file", "depth", "width", "height", "count"),
    [
        (Codec.H264, "h264-yuv420p.h264", 8, 256, 128, 36),
        (Codec.HEVC, "hevc-yuv420p.hevc", 8, 256, 128, 36),
        (Codec.HEVC, "hevc-main10.hevc", 10, 1280, 720, 1),
        (Codec.VP8, "vp8-yuv420p.ivf", 8, 256, 128, 36),
        (Codec.VP9, "vp9-yuv420p.ivf", 8, 256, 128, 36),
        (Codec.VP9, "vp9-yuv420p10le.ivf", 10, 256, 128, 36),
        (Codec.AV1, "av1-yuv420p.ivf", 8, 256, 128, 36),
        (Codec.AV1, "av1-yuv420p10le.ivf", 10, 256, 128, 36),
    ],
)
@pytest.mark.parametrize("capture_count", [2, 32])
@pytest.mark.parametrize("capture_delay", [0, 1])
def test_decode_all_pixels_timestamps_color_and_eos_use_mock_kernel(
    executable: Path,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    depth: int,
    width: int,
    height: int,
    count: int,
    capture_count: int,
    capture_delay: int,
) -> None:
    expected = accepted(frame_hashes(artifacts / file, depth, width, height))
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        result = accepted(
            play(
                process,
                codec,
                artifacts / file,
                capture_count=capture_count,
                capture_delay=capture_delay,
            )
        )
        assert result.eos and len(result.frames) == count
        # Stable-layout decoding must never spill pixels to auxiliary
        # surfaces. Ten-bit input has exactly one quantization per display;
        # eight-bit input is delivered from its registered CAPTURE storage.
        direct, quantized = (count, 0) if depth == 8 else (0, count)
        assert (
            f"direct={direct} quantized={quantized} unregistered_copy=0 "
            "held_copy=0 foreign_copy=0"
        ) in process.log()
        assert result.registrations == ((10, 1), (9, 1))
        assert 0 < result.maximum_reply_batch <= 16
        if count > 1:
            assert result.maximum_reply_batch > 1
        assert tuple(frame.digest for frame in result.frames) == expected
        assert result.layouts == ((width, height),)
        if codec in (Codec.VP8, Codec.VP9, Codec.AV1):
            assert result.reply_order.count(Reply.INPUT_DONE) == count
            assert [frame.timestamp for frame in result.frames] == [
                packet.timestamp * 1000
                for packet in accepted(load(codec, artifacts / file))
            ]
        if file == "hevc-main10.hevc":
            assert all(
                (frame.colorspace, frame.transfer, frame.ycbcr, frame.quantization)
                == (10, 7, 6, 2)
                for frame in result.frames
            )


@pytest.mark.parametrize(
    "codec,file,depth,width,height",
    [
        (Codec.H264, "h264-yuv420p.h264", 8, 256, 128),
        (Codec.HEVC, "hevc-yuv420p.hevc", 8, 256, 128),
        (Codec.HEVC, "hevc-main10.hevc", 10, 1280, 720),
        (Codec.VP8, "vp8-yuv420p.ivf", 8, 256, 128),
        (Codec.VP9, "vp9-yuv420p.ivf", 8, 256, 128),
        (Codec.VP9, "vp9-yuv420p10le.ivf", 10, 256, 128),
        (Codec.AV1, "av1-yuv420p.ivf", 8, 256, 128),
        (Codec.AV1, "av1-yuv420p10le.ivf", 10, 256, 128),
    ],
)
def test_decode_replaces_capture_started_before_first_packet(
    executable: Path,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    depth: int,
    width: int,
    height: int,
) -> None:
    expected = accepted(frame_hashes(artifacts / file, depth, width, height))
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        result = accepted(
            play(process, codec, artifacts / file, initial_capture_size=6144)
        )
        assert result.eos
        assert tuple(frame.digest for frame in result.frames) == expected
        assert result.layouts == ((width, height),)
        assert result.registrations == ((9, 1), (10, 1), (9, 2))


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
def test_decode_reuses_preallocated_capture_after_source_change(
    executable: Path,
    artifacts: Path,
    tmp_path: Path,
    codec: Codec,
    file: str,
    depth: int,
) -> None:
    expected = accepted(frame_hashes(artifacts / file, depth, 256, 128))
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        result = accepted(
            play(
                process,
                codec,
                artifacts / file,
                initial_capture_size=256 * 128 * 3,
                reuse_capture=True,
                capture_count=32,
            )
        )
        assert result.eos
        assert tuple(frame.digest for frame in result.frames) == expected
        assert result.registrations == ((9, 1), (10, 1))


@pytest.mark.parametrize(
    "codec,file", [(Codec.H264, "h264-yuv420p.h264"), (Codec.HEVC, "hevc-yuv420p.hevc")]
)
def test_decode_client_framed_access_units_progress_with_one_input_buffer(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
) -> None:
    expected = accepted(frame_hashes(artifacts / file, 8, 256, 128))
    result = accepted(play(mock_service, codec, artifacts / file, input_count=1))
    assert result.eos
    assert tuple(frame.digest for frame in result.frames) == expected
