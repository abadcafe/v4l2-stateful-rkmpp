"""module: tests.support.pixels_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_decode_png_reads_actual_rgb_pixels
    test_decode_png_invalid_or_truncated_input_is_rejected
    test_crop_rgb_observes_rows_without_padding
    test_crop_rgb_out_of_bounds_and_inconsistent_storage_are_rejected

Independent binary inputs define screenshot decoding and crop boundaries.
"""

import struct
import zlib

from tests.support.outcome import Rejected, accepted
from tests.support.pixels import Rgb, crop_rgb, decode_png


def _png(pixels: bytes) -> bytes:
    assert pixels and len(pixels) % 3 == 0

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data))
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">2I5B", len(pixels) // 3, 1, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(b"\x00" + pixels))
        + chunk(b"IEND", b"")
    )


def test_decode_png_reads_actual_rgb_pixels() -> None:
    png = _png(b"\xff\x00\x00\x00\xff\x00")
    image = accepted(decode_png(png))
    assert (image.width, image.height, image.pixels) == (
        2,
        1,
        b"\xff\x00\x00\x00\xff\x00",
    )


def test_decode_png_bounds_decoding_to_the_first_image() -> None:
    first = _png(b"\xff\x00\x00")
    following = _png(b"\x00\xff\x00")
    image = accepted(decode_png(first + following * 128))
    assert (image.width, image.height, image.pixels) == (1, 1, b"\xff\x00\x00")


def test_decode_png_invalid_or_truncated_input_is_rejected() -> None:
    for payload in (b"", b"not-png", b"\x89PNG\r\n\x1a\n"):
        assert isinstance(decode_png(payload), Rejected)


def test_crop_rgb_observes_rows_without_padding() -> None:
    image = Rgb(width=2, height=2, pixels=bytes(range(12)))
    assert accepted(crop_rgb(image, 1, 0, 1, 2)) == bytes([3, 4, 5, 9, 10, 11])


def test_crop_rgb_out_of_bounds_and_inconsistent_storage_are_rejected() -> None:
    image = Rgb(width=2, height=2, pixels=bytes(12))
    for rectangle in ((-1, 0, 1, 1), (0, 0, 0, 1), (1, 1, 2, 2)):
        assert isinstance(crop_rgb(image, *rectangle), Rejected)
    assert isinstance(
        crop_rgb(Rgb(width=2, height=2, pixels=b""), 0, 0, 1, 1), Rejected
    )
