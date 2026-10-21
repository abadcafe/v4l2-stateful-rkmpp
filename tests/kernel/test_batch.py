"""module: tests.kernel.test_batch; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_open_broker_is_exclusive_and_empty_accept_is_nonblocking
    test_exchange_rejects_invalid_header
    test_exchange_empty_batch_returns_no_events
    test_exchange_maximum_batch_preserves_packet_order
    test_exchange_invalid_reply_does_not_complete_input
    test_exchange_per_reply_errors_preserve_valid_reply_order


"""

import errno
from pathlib import Path

import pytest
from rkmpp_bridge_uapi import (
    ACCEPT,
    EXCHANGE,
    VERSION,
    Batch,
    Event,
    Message,
    Reply,
)

from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted, present
from tests.support.queue import Queue, QueueType
from tests.support.stream import Codec
from tests.support.syscalls import ioctl, open_fd
from tests.support.video_formats import set_format


def test_open_broker_is_exclusive_and_empty_accept_is_nonblocking(
    kernel_owner: Path,
) -> None:
    with accepted(open_fd(kernel_owner)) as broker:
        duplicate = open_fd(kernel_owner)
        assert isinstance(duplicate, Rejected) and duplicate.code == errno.EBUSY
        empty = ioctl(broker.fd, ACCEPT)
        assert isinstance(empty, Rejected) and empty.code == errno.EAGAIN


@pytest.mark.parametrize(
    ("version", "replies", "events", "reserved"),
    [
        pytest.param(VERSION + 1, 0, 0, 0, id="version"),
        pytest.param(VERSION, 17, 0, 0, id="reply-count-overflow"),
        pytest.param(VERSION, 0, 17, 0, id="event-count-overflow"),
        pytest.param(VERSION, 0, 0, 1, id="reserved"),
    ],
)
def test_exchange_rejects_invalid_header(
    raw_session: Peer, version: int, replies: int, events: int, reserved: int
) -> None:
    batch = Batch(
        version=version, reply_count=replies, event_count=events, reserved=reserved
    )
    result = ioctl(raw_session.service.fd, EXCHANGE, batch)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_exchange_empty_batch_returns_no_events(raw_session: Peer) -> None:
    batch = accepted(raw_session.service.exchange())
    assert (batch.version, batch.event_count, batch.reply_count) == (VERSION, 0, 0)


def test_exchange_maximum_batch_preserves_packet_order(raw_session: Peer) -> None:
    video = raw_session.video
    _ = accepted(set_format(video.fd, QueueType.OUTPUT, Codec.H264.fourcc, size=4096))
    output = Queue(video.fd, QueueType.OUTPUT)
    assert accepted(output.request(24)) == 24
    accepted(output.stream_on())
    for index in range(24):
        accepted(output.queue(index, 16, index * 100000))
    first = accepted(raw_session.service.exchange())
    second = accepted(raw_session.service.exchange())
    assert (first.event_count, second.event_count) == (16, 8)
    packets = [
        present(batch.event(i))
        for batch in (first, second)
        for i in range(batch.event_count)
    ]
    assert [p.index for p in packets] == list(range(24))
    # V4L2 timeval enters the bridge as vb2's nanosecond timestamp.
    assert all(
        p.kind == Event.PACKET
        and p.version == VERSION
        and p.timestamp == p.index * 100000000
        for p in packets
    )
    replies: list[Message] = []
    for message in packets[:16]:
        reply = message.clone()
        reply.kind = Reply.INPUT_DONE
        replies.append(reply)
    result = accepted(raw_session.service.exchange(replies))
    assert result.reply_count == 16 and list(present(result.results(16))) == [0] * 16
    assert [accepted(output.dequeue()).index for _ in range(16)] == list(range(16))


@pytest.mark.parametrize("field", ["version", "reserved", "pool_id", "epoch", "index"])
def test_exchange_invalid_reply_does_not_complete_input(
    packet_session: tuple[Peer, tuple[Message, ...]], field: str
) -> None:
    session, packets = packet_session
    reply = packets[0].clone()
    reply.kind = Reply.INPUT_DONE
    expected = (
        errno.EINVAL if field in ("version", "reserved", "index") else errno.ESTALE
    )
    if field == "reserved":
        reply.reserved = 1
    elif field == "index":
        reply.index = 32
    elif field == "version":
        reply.version += 1
    elif field == "pool_id":
        reply.pool_id += 1
    elif field == "epoch":
        reply.epoch += 1
    else:
        assert False, field
    result = accepted(session.service.exchange([reply]))
    assert present(result.result(0)) == -expected
    empty = Queue(session.video.fd, QueueType.OUTPUT).dequeue()
    assert isinstance(empty, Rejected) and empty.code == errno.EAGAIN


def test_exchange_per_reply_errors_preserve_valid_reply_order(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    session, packets = packet_session
    replies = [p.clone() for p in packets]
    for reply in replies:
        reply.kind = Reply.INPUT_DONE
    replies[1].epoch += 1
    batch = accepted(session.service.exchange(replies))
    assert list(present(batch.results(3))) == [0, -errno.ESTALE, 0]
    output = Queue(session.video.fd, QueueType.OUTPUT)
    assert [accepted(output.dequeue()).index for _ in range(2)] == [0, 2]
    replies[1].epoch = packets[1].epoch
    retry = accepted(session.service.exchange([replies[1]]))
    assert present(retry.result(0)) == 0 and accepted(output.dequeue()).index == 1


def test_exchange_empty_batch_preserves_unused_payload_storage(
    raw_session: Peer,
) -> None:
    payload = present(Batch.decode(b"\xa5" * Batch.SIZE))
    payload.version = VERSION
    payload.reply_count = 0
    payload.event_count = 0
    payload.reserved = 0
    _ = accepted(ioctl(raw_session.service.fd, EXCHANGE, payload))
    assert payload.event_count == 0
    assert bytes(payload)[16:] == b"\xa5" * (Batch.SIZE - 16)
