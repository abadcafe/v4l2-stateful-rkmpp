"""module: tests.support.stream_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_parse_ivf_preserves_payloads_and_converts_timebase
    test_parse_annex_b_preserves_complete_compressed_input
    test_parse_rejects_malformed_ivf
    test_load_missing_input_returns_errno_before_device_open
    test_load_valid_input_uses_same_packet_contract
    test_codec_fourcc_matches_published_v4l2_values


"""

import errno
import struct
from pathlib import Path

import pytest

from tests.support.outcome import Ok, Rejected, accepted
from tests.support.stream import Codec, load, parse


@pytest.mark.parametrize("codec", [Codec.VP8, Codec.VP9, Codec.AV1])
def test_parse_ivf_preserves_payloads_and_converts_timebase(codec: Codec) -> None:
    header = struct.pack(
        "<4sHH4sHHIIII", b"DKIF", 0, 32, codec.value.encode(), 256, 128, 10, 1, 2, 0
    )
    data = (
        header + struct.pack("<IQ", 3, 0) + b"one" + struct.pack("<IQ", 3, 5) + b"two"
    )
    packets = accepted(parse(codec, data))
    assert [(p.data, p.timestamp) for p in packets] == [(b"one", 0), (b"two", 500000)]


@pytest.mark.parametrize(
    "codec,file,first_size",
    [(Codec.H264, "h264-yuv420p.h264", 4454), (Codec.HEVC, "hevc-yuv420p.hevc", 5496)],
)
def test_parse_annex_b_produces_complete_client_access_units(
    codec: Codec, file: str, first_size: int
) -> None:
    data = (Path(__file__).parents[1] / "artifacts" / file).read_bytes()
    packets = accepted(parse(codec, data))
    assert len(packets) == 36
    assert len(packets[0].data) == first_size
    assert b"".join(packet.data for packet in packets) == data
    assert all(packet.timestamp == 0 for packet in packets)


@pytest.mark.parametrize(
    "data",
    [
        pytest.param(b"", id="empty"),
        pytest.param(b"DKIF", id="truncated-header"),
        pytest.param(
            struct.pack(
                "<4sHH4sHHIIII", b"DKIF", 1, 32, b"VP90", 256, 128, 10, 1, 0, 0
            ),
            id="version",
        ),
        pytest.param(
            struct.pack(
                "<4sHH4sHHIIII", b"DKIF", 0, 31, b"VP90", 256, 128, 10, 1, 0, 0
            ),
            id="header-length",
        ),
        pytest.param(
            struct.pack(
                "<4sHH4sHHIIII", b"DKIF", 0, 32, b"AV01", 256, 128, 10, 1, 0, 0
            ),
            id="codec",
        ),
        pytest.param(
            struct.pack("<4sHH4sHHIIII", b"DKIF", 0, 32, b"VP90", 256, 128, 0, 1, 0, 0),
            id="rate",
        ),
        pytest.param(
            struct.pack(
                "<4sHH4sHHIIII", b"DKIF", 0, 32, b"VP90", 256, 128, 10, 0, 0, 0
            ),
            id="scale",
        ),
        pytest.param(
            struct.pack(
                "<4sHH4sHHIIII", b"DKIF", 0, 32, b"VP90", 256, 128, 10, 1, 0, 0
            ),
            id="no-frames",
        ),
        pytest.param(
            struct.pack("<4sHH4sHHIIII", b"DKIF", 0, 32, b"VP90", 256, 128, 10, 1, 1, 0)
            + b"short",
            id="truncated-frame-header",
        ),
        pytest.param(
            struct.pack("<4sHH4sHHIIII", b"DKIF", 0, 32, b"VP90", 256, 128, 10, 1, 1, 0)
            + struct.pack("<IQ", 3, 0)
            + b"ab",
            id="truncated-payload",
        ),
        pytest.param(
            struct.pack("<4sHH4sHHIIII", b"DKIF", 0, 32, b"VP90", 256, 128, 10, 1, 1, 0)
            + struct.pack("<IQ", 0, 0),
            id="empty-payload",
        ),
        pytest.param(
            struct.pack("<4sHH4sHHIIII", b"DKIF", 0, 32, b"VP90", 256, 128, 10, 1, 2, 0)
            + struct.pack("<IQ", 1, 0)
            + b"x",
            id="count",
        ),
    ],
)
def test_parse_rejects_malformed_ivf(data: bytes) -> None:
    result = parse(Codec.VP9, data)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_load_missing_input_returns_errno_before_device_open(tmp_path: Path) -> None:
    result = load(Codec.VP9, tmp_path / "missing.ivf")
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_load_valid_input_uses_same_packet_contract(tmp_path: Path) -> None:
    path = tmp_path / "sample.h264"
    _ = path.write_bytes(b"\x00\x00\x01sample")
    result = load(Codec.H264, path)
    assert isinstance(result, Ok) and result.value[0].data == path.read_bytes()


def test_codec_fourcc_matches_published_v4l2_values() -> None:
    assert [codec.fourcc.to_bytes(4, "little") for codec in Codec] == [
        b"H264",
        b"HEVC",
        b"VP80",
        b"VP90",
        b"AV01",
    ]
