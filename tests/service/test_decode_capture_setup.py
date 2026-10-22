"""module: test_decode_capture_setup; responsibility: specify initial CAPTURE admission.

operations: pytest cases; entities: none; tests: replacement and immediate reuse
after the first source change, with real MPP/Vulkan execution.
"""

from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec


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
