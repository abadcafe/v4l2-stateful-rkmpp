"""module: tests.support.metadata_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_inspect_reports_actual_bit_depth_and_extent
    test_inspect_hdr_sources_are_ten_bit_bt2020
    test_inspect_missing_stream_is_rejected
    test_inspect_invalid_inline_stream_is_rejected

Input metadata proves each case actually exercises its claimed bit depth.
"""

from pathlib import Path

import pytest

from tests.support.metadata import inspect
from tests.support.outcome import Rejected, accepted


@pytest.mark.parametrize(
    ("file", "pixels", "width", "height"),
    [
        ("h264-yuv420p.h264", "yuv420p", 256, 128),
        ("hevc-yuv420p.hevc", "yuv420p", 256, 128),
        ("hevc-main10.hevc", "yuv420p10le", 1280, 720),
        ("vp8-yuv420p.ivf", "yuv420p", 256, 128),
        ("vp9-yuv420p.ivf", "yuv420p", 256, 128),
        ("vp9-yuv420p10le.ivf", "yuv420p10le", 256, 128),
        ("av1-yuv420p.ivf", "yuv420p", 256, 128),
        ("av1-yuv420p10le.ivf", "yuv420p10le", 256, 128),
    ],
)
def test_inspect_reports_actual_bit_depth_and_extent(
    artifacts: Path, file: str, pixels: str, width: int, height: int
) -> None:
    info = accepted(inspect(artifacts / file))
    assert (info.pix_fmt, info.width, info.height) == (pixels, width, height)


@pytest.mark.parametrize(
    ("file", "transfer"),
    [
        ("accept-hdr-pq-vp9.webm", "smpte2084"),
        ("accept-hdr-pq-av1.webm", "smpte2084"),
        ("accept-hdr-hlg-vp9.webm", "arib-std-b67"),
        ("accept-hdr-hlg-av1.webm", "arib-std-b67"),
    ],
)
def test_inspect_hdr_sources_are_ten_bit_bt2020(
    artifacts: Path, file: str, transfer: str
) -> None:
    info = accepted(inspect(artifacts / file))
    assert (info.pix_fmt, info.color_primaries, info.color_transfer) == (
        "yuv420p10le",
        "bt2020",
        transfer,
    )


def test_inspect_missing_stream_is_rejected(tmp_path: Path) -> None:
    assert isinstance(inspect(tmp_path / "missing.ivf"), Rejected)


def test_inspect_invalid_inline_stream_is_rejected(tmp_path: Path) -> None:
    path = tmp_path / "invalid.ivf"
    _ = path.write_bytes(b"not a stream")
    assert isinstance(inspect(path), Rejected)
