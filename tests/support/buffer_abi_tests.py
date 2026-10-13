"""module: tests.support.buffer_abi_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_buffer_wire_layout_matches_linux_aarch64_abi
    test_buffer_fields_roundtrip_through_public_wire_bytes


"""

import ctypes

import pytest

from tests.support.buffer_abi import (
    BufferMemory,
    ExportBuffer,
    Plane,
    PlaneMemory,
    Timecode,
    Timeval,
    VideoBuffer,
)
from tests.support.outcome import accepted
from tests.support.uapi import verify


def test_buffer_wire_layout_matches_linux_aarch64_abi() -> None:
    assert [
        ctypes.sizeof(t)
        for t in (
            Timeval,
            PlaneMemory,
            Plane,
            Timecode,
            BufferMemory,
            VideoBuffer,
            ExportBuffer,
        )
    ] == [16, 8, 64, 16, 8, 88, 64]


def test_buffer_fields_roundtrip_through_public_wire_bytes() -> None:
    payload = VideoBuffer(index=7, kind=10, memory=1, length=1)
    payload.timestamp.seconds = 123
    payload.timestamp.microseconds = 456
    copy = VideoBuffer.from_buffer_copy(bytes(payload))
    assert (
        copy.index,
        copy.kind,
        copy.memory,
        copy.length,
        copy.timestamp.seconds,
        copy.timestamp.microseconds,
    ) == (7, 10, 1, 1, 123, 456)


@pytest.mark.parametrize(
    ("wire_type", "c_type", "fields"),
    [
        (Timeval, "timeval", (("seconds", "tv_sec"), ("microseconds", "tv_usec"))),
        (Timecode, "v4l2_timecode", ()),
        (
            Plane,
            "v4l2_plane",
            (
                ("memory", "m"),
                ("bytesused", "bytesused"),
                ("length", "length"),
                ("data_offset", "data_offset"),
                ("reserved", "reserved"),
            ),
        ),
        (
            VideoBuffer,
            "v4l2_buffer",
            (
                ("kind", "type"),
                ("location", "m"),
                ("index", "index"),
                ("bytesused", "bytesused"),
                ("flags", "flags"),
                ("field", "field"),
                ("timestamp", "timestamp"),
                ("timecode", "timecode"),
                ("sequence", "sequence"),
                ("memory", "memory"),
                ("length", "length"),
                ("reserved2", "reserved2"),
                ("request_fd", "request_fd"),
            ),
        ),
        (
            ExportBuffer,
            "v4l2_exportbuffer",
            (
                ("kind", "type"),
                ("index", "index"),
                ("plane", "plane"),
                ("flags", "flags"),
                ("fd", "fd"),
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
