"""module: tests.kernel.test_layout_reply; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_layout_reply_rejects_invalid_geometry_without_source_change
    test_layout_reply_publishes_visible_extent_storage_and_colorimetry

Published layout replies validate geometry before exposing CAPTURE layouts.
"""

import ctypes
import errno

import pytest
from rkmpp_bridge_uapi import VERSION, Message, Reply

from tests.support.format_abi import Format
from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted, present
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import command, ioctl
from tests.support.video_formats import get_format, selection


@pytest.mark.parametrize(
    ("width", "height", "stride", "rows", "size", "format"),
    [
        (63, 64, 64, 64, 6144, 1),
        (8193, 64, 8193, 64, 786528, 1),
        (64, 63, 64, 64, 6144, 1),
        (64, 8193, 64, 8194, 786624, 1),
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
        coded_width=width,
        coded_height=height,
        version=VERSION,
        kind=Reply.LAYOUT,
        count=18,
        epoch=packets[0].epoch,
        width=width,
        height=height,
        stride=stride,
        rows=rows,
        size=size,
        format=format,
    )
    batch = accepted(session.service.exchange([reply]))
    assert present(batch.result(0)) == -errno.EINVAL
    assert accepted(session.video.events()) == []
    assert accepted(get_format(session.video.fd, QueueType.CAPTURE)).width == 64


def test_layout_reply_publishes_visible_extent_storage_and_colorimetry(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    session, packets = packet_session
    accepted(session.video.subscribe(5))
    reply = Message(
        coded_width=256,
        coded_height=128,
        version=VERSION,
        kind=Reply.LAYOUT,
        count=18,
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
    assert present(accepted(session.service.exchange([reply])).result(0)) == 0
    assert accepted(session.video.events()) == [(5, 1)]
    fmt = accepted(get_format(session.video.fd, QueueType.CAPTURE))
    assert (fmt.width, fmt.height, fmt.planes, fmt.strides, fmt.sizes) == (
        320,
        144,
        1,
        (320,),
        (69120,),
    )
    assert (fmt.colorspace, fmt.transfer, fmt.ycbcr, fmt.quantization) == (10, 7, 6, 2)
    assert accepted(selection(session.video.fd, QueueType.CAPTURE)) == (256, 128)


def test_layout_accepts_odd_visible_extent_and_updates_output(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    session, packets = packet_session
    reply = Message(
        coded_width=65,
        coded_height=65,
        version=VERSION,
        kind=Reply.LAYOUT,
        count=18,
        epoch=packets[0].epoch,
        width=65,
        height=65,
        stride=128,
        rows=80,
        size=15360,
        format=1,
    )
    assert present(accepted(session.service.exchange([reply])).result(0)) == 0
    assert accepted(selection(session.video.fd, QueueType.CAPTURE)) == (65, 65)
    output = accepted(get_format(session.video.fd, QueueType.OUTPUT))
    assert (output.width, output.height) == (65, 65)


@pytest.mark.parametrize(
    "target,expected",
    [
        (0, (0, 0, 65, 65)),
        (1, (0, 0, 65, 65)),
        (2, (0, 0, 65, 65)),
        (0x100, (0, 0, 65, 65)),
        (0x101, (0, 0, 65, 65)),
        (0x102, (0, 0, 65, 65)),
        (0x103, (0, 0, 128, 80)),
    ],
)
def test_selection_distinguishes_visible_coded_and_padded_rectangles(
    packet_session: tuple[Peer, tuple[Message, ...]],
    target: int,
    expected: tuple[int, int, int, int],
) -> None:
    import ctypes

    from tests.support.format_abi import Selection
    from tests.support.syscalls import command, ioctl

    peer, packets = packet_session
    layout = Message(
        coded_width=65,
        coded_height=65,
        version=VERSION,
        kind=Reply.LAYOUT,
        count=18,
        epoch=packets[0].epoch,
        width=65,
        height=65,
        stride=128,
        rows=80,
        size=15360,
        format=1,
    )
    assert present(accepted(peer.service.exchange([layout])).result(0)) == 0
    query = Selection(kind=QueueType.CAPTURE, target=target)
    assert (
        accepted(ioctl(peer.video.fd, command(3, "V", 94, ctypes.sizeof(query)), query))
        == 0
    )
    assert (
        query.rect.left,
        query.rect.top,
        query.rect.width,
        query.rect.height,
    ) == expected


def test_layout_allows_visible_crop_smaller_than_coded_minimum(
    raw_session: Peer,
) -> None:
    reply = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        count=18,
        epoch=1,
        width=32,
        height=32,
        coded_width=64,
        coded_height=64,
        crop_left=16,
        crop_top=16,
        stride=64,
        rows=64,
        size=6144,
        format=1,
    )
    assert present(accepted(raw_session.service.exchange([reply])).result(0)) == 0


@pytest.mark.parametrize("wire,field", [(0, 1), (1, 8), (2, 9), (3, 7), (4, 7), (5, 4)])
def test_layout_reports_actual_field_structure(
    raw_session: Peer, wire: int, field: int
) -> None:
    layout = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        count=18,
        epoch=1,
        width=64,
        height=64,
        coded_width=64,
        coded_height=64,
        stride=64,
        rows=64,
        size=6144,
        format=1,
        field=wire,
    )
    assert present(accepted(raw_session.service.exchange([layout])).result(0)) == 0
    payload = Format(kind=QueueType.CAPTURE)
    _ = accepted(
        ioctl(raw_session.video.fd, command(3, "V", 4, ctypes.sizeof(payload)), payload)
    )
    assert int.from_bytes(bytes(payload.fmt.data)[12:16], "little") == field


@pytest.mark.parametrize("minimum", [2, 18, 24, 32])
def test_layout_updates_capture_minimum_and_allocation(
    packet_session: tuple[Peer, tuple[Message, ...]],
    minimum: int,
) -> None:
    from tests.support.control_abi import Control

    session, packets = packet_session
    reply = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        epoch=packets[0].epoch,
        count=minimum,
        coded_width=64,
        coded_height=64,
        width=64,
        height=64,
        stride=64,
        rows=64,
        size=6144,
        format=1,
    )
    assert present(accepted(session.service.exchange([reply])).result(0)) == 0
    value = Control(id=0x980927)
    assert (
        accepted(
            ioctl(session.video.fd, command(3, "V", 27, ctypes.sizeof(value)), value)
        )
        == 0
    )
    assert value.value == minimum
    assert accepted(Queue(session.video.fd, QueueType.CAPTURE).request(1)) == minimum


def test_layout_larger_buffer_count_prevents_reusing_smaller_pool(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    from rkmpp_bridge_uapi import Event

    from tests.support.outcome import Rejected

    session, packets = packet_session
    capture = Queue(session.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    while accepted(session.service.exchange()).event_count:
        pass
    reply = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        epoch=packets[0].epoch,
        count=32,
        coded_width=64,
        coded_height=64,
        width=64,
        height=64,
        stride=64,
        rows=64,
        size=6144,
        format=1,
    )
    assert present(accepted(session.service.exchange([reply])).result(0)) == 0
    last = accepted(capture.dequeue())
    assert last.flags & 0x100000 and last.bytesused == (0,)
    result = session.video.start()
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    events = accepted(session.service.exchange())
    assert all(
        present(events.event(index)).kind != Event.CAPTURE
        for index in range(events.event_count)
    )
