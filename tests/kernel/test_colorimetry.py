"""module: test_colorimetry; responsibility: specify colorimetry admission.

operations: pytest cases; entities: none; tests: declared enum domains,
no truncation of invalid service metadata, normalized client format requests.
"""

import ctypes
import errno
import struct

import pytest
from rkmpp_bridge_uapi import VERSION, Message, Reply

from tests.support.format_abi import Format
from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted, present
from tests.support.queue import QueueType
from tests.support.syscalls import command, ioctl
from tests.support.video_formats import get_format


@pytest.mark.parametrize(
    "field,maximum",
    [("colorspace", 12), ("xfer_func", 7), ("ycbcr_enc", 8), ("quantization", 2)],
)
def test_layout_rejects_color_outside_declared_enum_before_publication(
    raw_session: Peer, field: str, maximum: int
) -> None:
    accepted(raw_session.video.subscribe(5))
    for value in (maximum + 1, 255, 256, 0xFFFFFFFF):
        fields = {field: value}
        layout = Message(
            version=VERSION,
            kind=Reply.LAYOUT,
            epoch=1,
            count=18,
            width=128,
            height=64,
            coded_width=128,
            coded_height=64,
            stride=128,
            rows=64,
            size=12288,
            format=1,
            **fields,
        )
        assert present(accepted(raw_session.service.exchange([layout])).result(0)) == (
            -errno.EINVAL
        )
        assert accepted(raw_session.video.events()) == []
        assert accepted(get_format(raw_session.video.fd, QueueType.CAPTURE)).width == 64
    # The inclusive upper enum boundary is valid and has not lost its generation.
    fields = {field: maximum}
    layout = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        epoch=1,
        count=18,
        width=128,
        height=64,
        coded_width=128,
        coded_height=64,
        stride=128,
        rows=64,
        size=12288,
        format=1,
        **fields,
    )
    assert present(accepted(raw_session.service.exchange([layout])).result(0)) == 0
    assert accepted(raw_session.video.events()) == [(5, 1)]


@pytest.mark.parametrize("operation", [5, 64], ids=["set", "try"])
def test_format_normalizes_undefined_color_enum_values(
    raw_session: Peer, operation: int
) -> None:
    payload = Format(kind=QueueType.OUTPUT)
    data = bytearray(200)
    struct.pack_into("<5I", data, 0, 128, 64, int.from_bytes(b"H264", "little"), 0, 3)
    data[180] = 1
    data[182:185] = bytes([255, 255, 255])
    payload.fmt.data[:] = data
    _ = accepted(
        ioctl(
            raw_session.video.fd,
            command(3, "V", operation, ctypes.sizeof(payload)),
            payload,
        )
    )
    assert bytes(payload.fmt.data)[182:185] == bytes(3)
