"""module: tests.kernel.test_frame_reply; responsibility: define the tested public contract.

operations: pytest cases; entities: CaptureSession; tests:
    test_frame_reply_completes_once_preserves_timestamp_and_recycles
    test_frame_reply_rejection_preserves_queued_capture_slot
    test_eos_reply_marks_last_capture_and_delivers_eos_event
    test_failed_reply_poisoning_rejects_new_buffer_allocation

Frame and terminal replies obey ownership, generations and completion order.
"""

import errno
from collections.abc import Generator
from dataclasses import dataclass

import pytest

from tests.support.bridge_abi import VERSION, Event, Message, Reply
from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType


@dataclass(frozen=True)
class CaptureSession:
    raw: Peer
    queue: Queue
    event: Message


@pytest.fixture
def capture_session(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> Generator[CaptureSession]:
    session, packets = packet_session
    accepted(session.video.subscribe(5))
    accepted(session.video.subscribe(2))
    layout = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        epoch=packets[0].epoch,
        width=64,
        height=64,
        stride=64,
        rows=64,
        size=6144,
        format=1,
    )
    assert accepted(session.service.exchange([layout])).results[0] == 0
    assert accepted(session.video.events()) == [(5, 1)]
    queue = Queue(session.video.fd, QueueType.CAPTURE)
    assert accepted(queue.request(18)) == 18
    for index in range(18):
        accepted(queue.queue(index))
    accepted(queue.stream_on())
    events = accepted(session.service.exchange())
    assert events.event_count == 16
    event = Message.from_buffer_copy(events.events)
    assert event.kind == Event.CAPTURE
    assert event.count == 18 and event.generation > 0
    assert accepted(session.service.exchange()).event_count == 3
    yield CaptureSession(session, queue, event)


def frame(event: Message) -> Message:
    reply = Message.from_buffer_copy(event)
    reply.kind = Reply.FRAME
    reply.bytesused = 6144
    reply.timestamp = 323456000
    return reply


def test_frame_reply_completes_once_preserves_timestamp_and_recycles(
    capture_session: CaptureSession,
) -> None:
    session = capture_session
    reply = frame(session.event)
    assert accepted(session.raw.service.exchange([reply])).results[0] == 0
    assert accepted(session.raw.service.exchange([reply])).results[0] == -errno.ESTALE
    done = accepted(session.queue.dequeue())
    assert (done.index, done.sequence, done.timestamp, done.bytesused) == (
        0,
        0,
        323456,
        (6144,),
    )
    accepted(session.queue.queue(done.index))
    event = Message.from_buffer_copy(accepted(session.raw.service.exchange()).events)
    assert (
        event.kind == Event.RECYCLE
        and event.index == 0
        and event.pool_id == reply.pool_id
    )
    reply.timestamp += 1000000
    assert accepted(session.raw.service.exchange([reply])).results[0] == 0
    assert accepted(session.queue.dequeue()).sequence == 1


@pytest.mark.parametrize("field", ["pool", "generation", "index", "payload"])
def test_frame_reply_rejection_preserves_queued_capture_slot(
    capture_session: CaptureSession, field: str
) -> None:
    session = capture_session
    reply = frame(session.event)
    if field == "pool":
        reply.pool_id += 1
    elif field == "generation":
        reply.generation += 1
    elif field == "index":
        reply.index = 32
    else:
        reply.bytesused = 6145
    assert accepted(session.raw.service.exchange([reply])).results[0] == -(
        errno.EINVAL if field == "payload" else errno.ESTALE
    )
    empty = session.queue.dequeue()
    assert isinstance(empty, Rejected) and empty.code == errno.EAGAIN
    assert (
        accepted(session.raw.service.exchange([frame(session.event)])).results[0] == 0
    )
    assert accepted(session.queue.dequeue()).index == 0


def test_eos_reply_marks_last_capture_and_delivers_eos_event(
    capture_session: CaptureSession,
) -> None:
    session = capture_session
    eos = Message.from_buffer_copy(session.event)
    eos.kind = Reply.EOS
    assert accepted(session.raw.service.exchange([frame(session.event), eos])).results[
        :2
    ] == [0, 0]
    first, last = accepted(session.queue.dequeue()), accepted(session.queue.dequeue())
    assert first.bytesused == (6144,) and last.bytesused == (0,)
    assert last.flags & 0x100000
    assert accepted(session.raw.video.events()) == [(2, 0)]


def test_failed_reply_poisoning_rejects_new_buffer_allocation(
    capture_session: CaptureSession,
) -> None:
    session = capture_session
    failed = Message.from_buffer_copy(session.event)
    failed.kind = Reply.FAILED
    terminal = session.raw.service.exchange([failed])
    assert isinstance(terminal, Rejected) and terminal.code == errno.EPIPE
    output = Queue(session.raw.video.fd, QueueType.OUTPUT)
    accepted(output.stream_off())
    _ = accepted(output.request(0))
    result = output.request(3)
    assert isinstance(result, Rejected) and result.code == errno.EIO
