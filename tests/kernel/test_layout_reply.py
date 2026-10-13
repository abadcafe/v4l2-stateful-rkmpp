"""module: tests.kernel.test_layout_reply; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_layout_reply_rejects_invalid_geometry_without_source_change
    test_layout_reply_publishes_visible_extent_storage_and_colorimetry

Published layout replies validate geometry before exposing CAPTURE layouts.
"""

import errno

import pytest

from tests.support.bridge_abi import VERSION, Message, Reply
from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted
from tests.support.queue import QueueType
from tests.support.video_formats import get_format, selection


@pytest.mark.parametrize(
    ("width", "height", "stride", "rows", "size", "format"),
    [
        (63, 64, 64, 64, 6144, 1),
        (8193, 64, 8193, 64, 786528, 1),
        (65, 64, 128, 64, 12288, 1),
        (64, 63, 64, 64, 6144, 1),
        (64, 8193, 64, 8194, 786624, 1),
        (64, 65, 64, 66, 6336, 1),
        (64, 64, 64, 62, 6144, 1),
        (64, 64, 64, 65, 6240, 1),
        (64, 64, 64, 16386, 1573056, 1),
        (64, 64, 63, 64, 6144, 1),
        (64, 64, 65537, 64, 6291552, 1),
        (64, 64, 64, 64, 6143, 1),
        (64, 64, 64, 64, 134217729, 1),
        (64, 64, 64, 64, 6144, 99),
    ],
)
def test_layout_reply_rejects_invalid_geometry_without_source_change(
    packet_session: tuple[Peer, tuple[Message, ...]],
    width: int,
    height: int,
    stride: int,
    rows: int,
    size: int,
    format: int,
) -> None:
    session, packets = packet_session
    accepted(session.video.subscribe(5))
    reply = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        epoch=packets[0].epoch,
        width=width,
        height=height,
        stride=stride,
        rows=rows,
        size=size,
        format=format,
    )
    batch = accepted(session.service.exchange([reply]))
    assert batch.results[0] == -errno.EINVAL
    assert accepted(session.video.events()) == []
    assert accepted(get_format(session.video.fd, QueueType.CAPTURE)).width == 64


def test_layout_reply_publishes_visible_extent_storage_and_colorimetry(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    session, packets = packet_session
    accepted(session.video.subscribe(5))
    reply = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        epoch=packets[0].epoch,
        width=256,
        height=128,
        stride=320,
        rows=144,
        size=69120,
        format=1,
        colorspace=10,
        xfer_func=7,
        ycbcr_enc=6,
        quantization=2,
    )
    assert accepted(session.service.exchange([reply])).results[0] == 0
    assert accepted(session.video.events()) == [(5, 1)]
    fmt = accepted(get_format(session.video.fd, QueueType.CAPTURE))
    assert (fmt.width, fmt.height, fmt.planes, fmt.strides, fmt.sizes) == (
        256,
        144,
        1,
        (320,),
        (69120,),
    )
    assert (fmt.colorspace, fmt.transfer, fmt.ycbcr, fmt.quantization) == (10, 7, 6, 2)
    assert accepted(selection(session.video.fd, QueueType.CAPTURE)) == (256, 128)
