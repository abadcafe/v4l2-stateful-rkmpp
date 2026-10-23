"""module: test_slice_admission; responsibility: reject invalid coded slice headers.

operations: process cases; entities: Damage; tests: initial/established H.264 and
HEVC pictures, invalid addresses/types, input damage and keyframe recovery.
"""

from pathlib import Path
from typing import Literal

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec, Packet, load

type Damage = Literal["address", "type"]


@pytest.mark.parametrize("positions", [(4,), (33,), (42,), (740, 3146)])
@pytest.mark.parametrize("established", [False, True])
def test_decode_rejects_invalid_nal_reference_flags_and_recovers(
    mock_service: Process,
    artifacts: Path,
    positions: tuple[int, ...],
    established: bool,
) -> None:
    source = artifacts / "h264-two-slices.h264"
    good = accepted(load(Codec.H264, source))
    damaged = bytearray(source.read_bytes())
    for position in positions:
        damaged[position] ^= 0x20 if position == 42 else 0x60
    preceding = good if established else ()
    result = accepted(
        play(
            mock_service,
            Codec.H264,
            source,
            input_count=1,
            submissions=(*preceding, Packet(data=bytes(damaged), timestamp=0), *good),
        )
    )
    assert result.eos and not result.errors
    assert tuple(bool(done.flags & 2) for done in result.input_completions) == (
        *(False for _ in preceding),
        True,
        *(False for _ in good),
    )
    expected = accepted(frame_hashes(source, 8, 128, 128))
    assert tuple(frame.digest for frame in result.frames) == expected * (
        2 if established else 1
    )


@pytest.mark.parametrize(
    "codec,file",
    [
        (Codec.H264, "h264-two-slices.h264"),
        (Codec.HEVC, "hevc-two-slices.hevc"),
    ],
)
@pytest.mark.parametrize("established", [False, True])
@pytest.mark.parametrize("damage", ["address", "type"])
def test_decode_rejects_invalid_slice_header_before_hardware_and_recovers(
    mock_service: Process,
    artifacts: Path,
    codec: Codec,
    file: str,
    established: bool,
    damage: Damage,
) -> None:
    source = artifacts / file
    good = accepted(load(codec, source))
    data = source.read_bytes()
    if codec == Codec.H264 and damage == "address":
        # Second slice: ue(32) -> ue(64), beyond the 8x8-macroblock picture.
        bits = "".join(f"{byte:08b}" for byte in data[3147:])
        assert bits[:11] == "00000100001"
        bits = f"{65:013b}" + bits[11:]
        damaged = data[:3147] + bytes(
            int(bits[index : index + 8].ljust(8, "0"), 2)
            for index in range(0, len(bits), 8)
        )
    elif codec == Codec.H264:
        altered = bytearray(data)
        assert altered[741] == 0x88
        altered[741] = 0x8D  # slice_type 7 -> 12, same Exp-Golomb width.
        damaged = bytes(altered)
    elif damage == "address":
        # Nonfirst HEVC segment: CTB address 8 -> 0.
        altered = bytearray(data)
        assert altered[5721] == 0x30
        altered[5721] &= ~0x10
        damaged = bytes(altered)
    else:
        # First independent segment: slice_type 2 -> 3, two additional bits.
        bits = "".join(f"{byte:08b}" for byte in data[2388:5716])
        assert bits[:6] == "101011"
        bits = bits[:3] + "00100" + bits[6:]
        damaged = (
            data[:2388]
            + bytes(
                int(bits[index : index + 8].ljust(8, "0"), 2)
                for index in range(0, len(bits), 8)
            )
            + data[5716:]
        )
    preceding = good if established else ()
    result = accepted(
        play(
            mock_service,
            codec,
            source,
            input_count=1,
            submissions=(*preceding, Packet(data=damaged, timestamp=0), *good),
        )
    )
    assert result.eos and not result.errors
    assert tuple(bool(done.flags & 2) for done in result.input_completions) == (
        *(False for _ in preceding),
        True,
        *(False for _ in good),
    )
    side = 128 if codec == Codec.H264 else 256
    expected = accepted(frame_hashes(source, 8, side, side))
    assert tuple(frame.digest for frame in result.frames) == expected * (
        2 if established else 1
    )
