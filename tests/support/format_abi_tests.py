"""module: tests.support.format_abi_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_format_wire_sizes_match_linux_aarch64_abi
    test_format_union_preserves_packed_pix_mp_bytes


"""

import ctypes

import pytest

from tests.support.format_abi import (
    Capability,
    Format,
    FormatDescription,
    FormatUnion,
    Rect,
    RequestBuffers,
    Selection,
)
from tests.support.outcome import accepted
from tests.support.uapi import verify


def test_format_wire_sizes_match_linux_aarch64_abi() -> None:
    assert [
        ctypes.sizeof(t)
        for t in (
            FormatUnion,
            Format,
            RequestBuffers,
            Rect,
            Selection,
            Capability,
            FormatDescription,
        )
    ] == [200, 208, 20, 16, 64, 104, 64]


def test_format_union_preserves_packed_pix_mp_bytes() -> None:
    payload = Format(kind=9)
    payload.fmt.data[180] = 1
    copy = Format.from_buffer_copy(bytes(payload))
    assert copy.kind == 9 and copy.fmt.data[180] == 1


@pytest.mark.parametrize(
    ("wire_type", "c_type", "fields"),
    [
        (Format, "v4l2_format", (("kind", "type"), ("fmt", "fmt"))),
        (
            RequestBuffers,
            "v4l2_requestbuffers",
            (
                ("kind", "type"),
                ("count", "count"),
                ("memory", "memory"),
                ("capabilities", "capabilities"),
                ("flags", "flags"),
                ("reserved", "reserved"),
            ),
        ),
        (
            Rect,
            "v4l2_rect",
            (
                ("left", "left"),
                ("top", "top"),
                ("width", "width"),
                ("height", "height"),
            ),
        ),
        (
            Selection,
            "v4l2_selection",
            (
                ("kind", "type"),
                ("rect", "r"),
                ("target", "target"),
                ("flags", "flags"),
                ("reserved", "reserved"),
            ),
        ),
        (
            Capability,
            "v4l2_capability",
            (
                ("driver", "driver"),
                ("card", "card"),
                ("bus_info", "bus_info"),
                ("version", "version"),
                ("capabilities", "capabilities"),
                ("device_caps", "device_caps"),
                ("reserved", "reserved"),
            ),
        ),
        (
            FormatDescription,
            "v4l2_fmtdesc",
            (
                ("kind", "type"),
                ("index", "index"),
                ("flags", "flags"),
                ("description", "description"),
                ("pixelformat", "pixelformat"),
                ("mbus_code", "mbus_code"),
                ("reserved", "reserved"),
            ),
        ),
    ],
)
def test_wire_all_fields_match_the_compiled_public_c_header(
    wire_type: type[ctypes.Structure],
    c_type: str,
    fields: tuple[tuple[str, str], ...],
) -> None:
    assert accepted(verify(wire_type, c_type, fields)) is None
