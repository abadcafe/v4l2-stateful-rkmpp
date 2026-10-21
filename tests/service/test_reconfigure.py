"""module: test_reconfigure; responsibility: specify process capture replacement.

operations: pytest cases; entities: none; tests: changing layouts preserve pixels.
"""

from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec


@pytest.mark.parametrize(
    ("codec", "file", "first", "second", "depth", "second_count"),
    [
        (Codec.H264, "h264-resize.h264", "h264-yuv420p.h264", "h264-512.h264", 8, 36),
        (
            Codec.VP9,
            "vp9-10-resize.ivf",
            "vp9-yuv420p10le.ivf",
            "vp9-10-resize-second.ivf",
            10,
            12,
        ),
        (
            Codec.AV1,
            "av1-10-resize.ivf",
            "av1-yuv420p10le.ivf",
            "av1-10-resize-second.ivf",
            10,
            12,
        ),
    ],
)
@pytest.mark.parametrize("reuse", [False, True])
def test_decode_changed_layout_preserves_every_pixel(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
    first: str,
    second: str,
    depth: int,
    second_count: int,
    reuse: bool,
) -> None:
    expected = accepted(frame_hashes(artifacts / first, depth, 256, 128)) + accepted(
        frame_hashes(artifacts / second, depth, 512, 256)
    )
    result = accepted(
        play(
            mock_service,
            codec,
            artifacts / file,
            initial_capture_size=512 * 256 * 3 if reuse else 0,
            reuse_capture=reuse,
            capture_count=32 if reuse else 2,
        )
    )
    assert all(frame.flags == 0 for frame in result.frames), [
        frame.flags for frame in result.frames
    ]
    assert result.eos and len(result.frames) == 36 + second_count
    assert len(result.frames) == len(expected)
    mismatches = [
        (index, frame.width, frame.height, frame.digest, digest)
        for index, (frame, digest) in enumerate(
            zip(result.frames, expected, strict=True)
        )
        if frame.digest != digest
    ]
    assert not mismatches, mismatches
    assert [(frame.width, frame.height) for frame in result.frames] == [
        (256, 128)
    ] * 36 + [(512, 256)] * second_count
    assert result.layouts == ((256, 128), (512, 256))
    assert result.registrations == (
        ((9, 1), (10, 1)) if reuse else ((10, 1), (9, 1), (9, 2))
    )
