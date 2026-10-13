"""module: test_decode; responsibility: specify real process codec output.

operations: pytest cases; entities: none; tests: decode pixel/timestamp/color/EOS.
The kernel endpoint is simulated; production MPP/Vulkan execution is real.
"""

from pathlib import Path

import pytest

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
) -> None:
    expected = accepted(frame_hashes(artifacts / file, depth, width, height))
    library = (
        executable.parent.parent / "kernel-mock/debug/libv4l2_rkmpp_kernel_mock.so"
    )
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        result = accepted(play(process, codec, artifacts / file))
        assert result.eos and len(result.frames) == count
        assert result.registrations == ((10, 1), (9, 1))
        assert 0 < result.maximum_reply_batch <= 16
        if count > 1:
            assert result.maximum_reply_batch > 1
        assert tuple(frame.digest for frame in result.frames) == expected
        assert result.layouts == ((width, height),)
        if codec in (Codec.VP8, Codec.VP9, Codec.AV1):
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
