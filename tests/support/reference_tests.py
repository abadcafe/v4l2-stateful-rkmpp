"""module: tests.support.reference_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_quantize_fractional_values_follow_spatial_thresholds
    test_quantize_saturates_and_preserves_frame_boundaries
    test_quantize_invalid_extent_or_truncated_planar_data_is_rejected
    test_frame_hashes_missing_reference_returns_rejected
    test_frame_hashes_known_sample_has_complete_independent_frames


"""

import errno
import struct
from pathlib import Path

from tests.support.outcome import Rejected, accepted
from tests.support.reference import frame_hashes, quantize


def test_quantize_fractional_values_follow_spatial_thresholds() -> None:
    raw = struct.pack("<24H", *([257] * 24))
    pixels = accepted(quantize(raw, 4, 4))
    assert pixels == bytes(
        [
            64,
            64,
            64,
            64,
            65,
            64,
            65,
            64,
            64,
            64,
            64,
            64,
            65,
            64,
            65,
            64,
            64,
            64,
            65,
            64,
            64,
            64,
            65,
            64,
        ]
    )


def test_quantize_saturates_and_preserves_frame_boundaries() -> None:
    raw = struct.pack("<12H", *([1023] * 6 + [0] * 6))
    assert accepted(quantize(raw, 2, 2)) == bytes([255] * 6 + [0] * 6)


def test_quantize_invalid_extent_or_truncated_planar_data_is_rejected() -> None:
    for raw, width, height in [(b"", 0, 2), (b"", 3, 2), (b"x", 2, 2)]:
        result = quantize(raw, width, height)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_quantize_odd_planes_restart_threshold_coordinates_and_clamp_large_samples() -> (
    None
):
    for width, height in ((1, 1), (3, 5), (8, 3)):
        dimensions = (
            (width, height),
            ((width + 1) // 2, (height + 1) // 2),
            ((width + 1) // 2, (height + 1) // 2),
        )
        samples: list[int] = []
        expected = bytearray()
        for _ in range(2):
            for columns, rows in dimensions:
                for row in range(rows):
                    for column in range(columns):
                        value = (0, 257, 1022, 1023, 65535)[(row + column) % 5]
                        samples.append(value)
                        expected.append(
                            min(
                                255,
                                (value + ((0, 2), (3, 1))[row % 2][column % 2]) // 4,
                            )
                        )
        raw = struct.pack(f"<{len(samples)}H", *samples)
        assert accepted(quantize(raw, width, height)) == bytes(expected)


def test_frame_hashes_missing_reference_returns_rejected(tmp_path: Path) -> None:
    assert isinstance(frame_hashes(tmp_path / "missing.h264", 8, 256, 128), Rejected)


def test_frame_hashes_known_sample_has_complete_independent_frames(
    artifacts: Path,
) -> None:
    hashes = accepted(frame_hashes(artifacts / "h264-yuv420p.h264", 8, 256, 128))
    assert len(hashes) == 36 and all(len(digest) == 64 for digest in hashes)
    assert len(set(hashes)) > 1
