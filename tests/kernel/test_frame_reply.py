"""module: tests.kernel.test_frame_reply; responsibility: define the tested public contract.

operations: pytest cases; entities: CaptureSession; tests:
    test_frame_reply_completes_once_preserves_timestamp_and_recycles
    test_frame_reply_rejection_preserves_queued_capture_slot
    test_eos_reply_marks_last_capture_and_delivers_eos_event
    test_failed_reply_requires_cleanup_of_both_queues

Frame and terminal replies obey ownership, generations and completion order.
"""

import ctypes
import errno
from collections.abc import Generator
from dataclasses import dataclass

import pytest
from rkmpp_bridge_uapi import VERSION, Event, Message, Reply

from tests.support.buffer_abi import Plane, VideoBuffer
from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted, present
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import command, ioctl


@dataclass(frozen=True)
class CaptureSession:
    raw: Peer
    queue: Queue
    queued: tuple[Message, ...]
    packets: tuple[Message, ...]


@pytest.fixture
def capture_session(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> Generator[CaptureSession]:
    session, packets = packet_session
    accepted(session.video.subscribe(5))
    accepted(session.video.subscribe(2))
    layout = Message(
        coded_width=64,
        coded_height=64,
        version=VERSION,
        kind=Reply.LAYOUT,
        count=18,
        epoch=packets[0].epoch,
        width=64,
        height=64,
        stride=64,
        rows=64,
        size=6144,
        format=1,
    )
    assert present(accepted(session.service.exchange([layout])).result(0)) == 0
    assert accepted(session.video.events()) == [(5, 1)]
    queue = Queue(session.video.fd, QueueType.CAPTURE)
    assert accepted(queue.request(18)) == 18
    for index in range(18):
        accepted(queue.queue(index))
    accepted(queue.stream_on())
    events = accepted(session.service.exchange())
    assert events.event_count == 16
    event = present(events.event(0))
    assert event.kind == Event.CAPTURE
    assert event.count == 18 and event.generation > 0
    tail = accepted(session.service.exchange())
    assert tail.event_count == 3
    queued = tuple(
        (
            present(batch.event(i))
            for batch, start in ((events, 1), (tail, 0))
            for i in range(start, batch.event_count)
        )
    )
    assert all(event.kind == Event.RECYCLE and event.count for event in queued)
    assert len({event.count for event in queued}) == 18
    yield CaptureSession(session, queue, queued, packets)


def frame(event: Message) -> Message:
    reply = event.clone()
    reply.kind = Reply.FRAME
    reply.bytesused = 6144
    reply.timestamp = 323456000
    return reply


def test_frame_reply_completes_once_preserves_timestamp_and_recycles(
    capture_session: CaptureSession,
) -> None:
    session = capture_session
    reply = frame(session.queued[0])
    assert present(accepted(session.raw.service.exchange([reply])).result(0)) == 0
    assert (
        present(accepted(session.raw.service.exchange([reply])).result(0))
        == -errno.ESTALE
    )
    done = accepted(session.queue.dequeue())
    assert (done.index, done.sequence, done.timestamp, done.bytesused) == (
        0,
        0,
        323456,
        (6144,),
    )
    accepted(session.queue.queue(done.index))
    event = present(accepted(session.raw.service.exchange()).event(0))
    assert (
        event.kind == Event.RECYCLE
        and event.index == 0
        and event.pool_id == reply.pool_id
    )
    reply.timestamp += 1000000
    assert (
        present(accepted(session.raw.service.exchange([reply])).result(0))
        == -errno.ESTALE
    )
    pending = session.queue.dequeue()
    assert isinstance(pending, Rejected) and pending.code == errno.EAGAIN
    reply.count = event.count
    assert present(accepted(session.raw.service.exchange([reply])).result(0)) == 0
    assert accepted(session.queue.dequeue()).sequence == 1


@pytest.mark.parametrize(
    "field", ["pool", "generation", "index", "token", "zero-token", "payload"]
)
def test_frame_reply_rejection_preserves_queued_capture_slot(
    capture_session: CaptureSession, field: str
) -> None:
    session = capture_session
    reply = frame(session.queued[0])
    if field == "pool":
        reply.pool_id += 1
    elif field == "generation":
        reply.generation += 1
    elif field == "index":
        reply.index = 32
    elif field == "token":
        reply.count += 1
    elif field == "zero-token":
        reply.count = 0
    else:
        reply.bytesused = 6145
    assert present(accepted(session.raw.service.exchange([reply])).result(0)) == -(
        errno.EINVAL if field == "payload" else errno.ESTALE
    )
    empty = session.queue.dequeue()
    assert isinstance(empty, Rejected) and empty.code == errno.EAGAIN
    assert (
        present(
            accepted(session.raw.service.exchange([frame(session.queued[0])])).result(0)
        )
        == 0
    )
    assert accepted(session.queue.dequeue()).index == 0


def test_eos_reply_marks_last_capture_and_delivers_eos_event(
    capture_session: CaptureSession,
) -> None:
    session = capture_session
    accepted(session.raw.video.stop())
    _ = accepted(session.raw.service.exchange())
    eos = session.queued[0].clone()
    eos.kind = Reply.EOS
    assert present(
        accepted(session.raw.service.exchange([frame(session.queued[0]), eos])).results(
            2
        )
    ) == [0, 0]
    first, last = accepted(session.queue.dequeue()), accepted(session.queue.dequeue())
    assert first.bytesused == (6144,) and last.bytesused == (0,)
    assert last.flags & 0x100000
    assert accepted(session.raw.video.events()) == [(2, 0)]


def test_failed_reply_requires_cleanup_of_both_queues(
    capture_session: CaptureSession,
) -> None:
    session = capture_session
    failed = session.queued[0].clone()
    failed.kind = Reply.FAILED
    terminal = session.raw.service.exchange([failed])
    assert isinstance(terminal, Rejected) and terminal.code == errno.EPIPE
    output = Queue(session.raw.video.fd, QueueType.OUTPUT)
    accepted(output.stream_off())
    _ = accepted(output.request(0))
    result = output.request(3)
    assert isinstance(result, Rejected) and result.code == errno.EIO


def test_damaged_frame_returns_error_without_poisoning_session(
    capture_session: CaptureSession,
) -> None:
    session = capture_session
    reply = frame(session.queued[0])
    reply.flags = 2
    assert present(accepted(session.raw.service.exchange([reply])).result(0)) == 0
    assert accepted(session.queue.dequeue()).flags & 0x40
    accepted(session.queue.queue(0))
    recycled = present(accepted(session.raw.service.exchange()).event(0))
    reply.count = recycled.count
    reply.flags = 0
    assert present(accepted(session.raw.service.exchange([reply])).result(0)) == 0
    assert accepted(session.queue.dequeue()).flags & 0x40 == 0


@pytest.mark.parametrize("wire,field", [(0, 1), (1, 8), (2, 9), (3, 2), (4, 3), (5, 4)])
def test_frame_dequeue_preserves_service_field_order(
    capture_session: CaptureSession, wire: int, field: int
) -> None:
    session = capture_session
    reply = frame(session.queued[0])
    reply.field = wire
    assert present(accepted(session.raw.service.exchange([reply])).result(0)) == 0
    planes = (Plane * 1)()
    payload = VideoBuffer(kind=QueueType.CAPTURE, memory=1, length=1)
    payload.location.planes = ctypes.addressof(planes)
    _ = accepted(
        ioctl(
            session.raw.video.fd, command(3, "V", 17, ctypes.sizeof(payload)), payload
        )
    )
    assert payload.field == field


@pytest.mark.parametrize("same_batch", [False, True])
def test_drain_completion_rejects_duplicate_eos_and_late_frame(
    capture_session: CaptureSession, same_batch: bool
) -> None:
    session = capture_session
    accepted(session.raw.video.stop())
    _ = accepted(session.raw.service.exchange())
    eos = session.queued[0].clone()
    eos.kind = Reply.EOS
    late = frame(session.queued[2])
    if same_batch:
        results = present(
            accepted(session.raw.service.exchange([eos, eos, late])).results(3)
        )
        assert list(results[:3]) == [0, -errno.EBUSY, -errno.EBUSY]
    else:
        assert present(accepted(session.raw.service.exchange([eos])).result(0)) == 0
        for message in (eos, late):
            assert (
                present(accepted(session.raw.service.exchange([message])).result(0))
                == -errno.EBUSY
            )
    last = accepted(session.queue.dequeue())
    assert last.index == 0 and last.flags & 0x100000
    assert accepted(session.raw.video.events()) == [(2, 0)]
    for index in (1, 2):
        assert not accepted(session.queue.query(index)).flags & 0x4
    end = session.queue.dequeue()
    assert isinstance(end, Rejected) and end.code == errno.EPIPE
    completions = [packet.clone() for packet in session.packets]
    for completion in completions:
        completion.kind = Reply.INPUT_DONE
    assert list(
        present(accepted(session.raw.service.exchange(completions)).results(3))
    ) == [
        0,
        0,
        0,
    ]
    output = Queue(session.raw.video.fd, QueueType.OUTPUT)
    for _ in completions:
        _ = accepted(output.dequeue())
    accepted(session.raw.video.start())
    _ = accepted(session.raw.service.exchange())
    empty = session.queue.dequeue()
    assert isinstance(empty, Rejected) and empty.code == errno.EAGAIN
    assert present(accepted(session.raw.service.exchange([late])).result(0)) == 0
    done = accepted(session.queue.dequeue())
    assert done.index == 2 and done.bytesused == (6144,) and not done.flags & 0x100000


def test_eos_requires_an_explicit_or_natural_drain(
    capture_session: CaptureSession,
) -> None:
    session = capture_session
    eos = session.queued[0].clone()
    eos.kind = Reply.EOS
    assert (
        present(accepted(session.raw.service.exchange([eos])).result(0)) == -errno.EBUSY
    )
    assert accepted(session.raw.video.events()) == []
    assert (
        present(
            accepted(session.raw.service.exchange([frame(session.queued[0])])).result(0)
        )
        == 0
    )
    assert accepted(session.queue.dequeue()).bytesused == (6144,)
