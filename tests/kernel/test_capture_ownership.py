"""module: test_capture_ownership; responsibility: specify retained CAPTURE ownership.

operations: pytest cases; entities: none; tests: retained CAPTURE snapshots,
queued storage before OUTPUT streaming and LAST recovery.
"""

import time
from concurrent.futures import ThreadPoolExecutor

import pytest
from rkmpp_bridge_uapi import VERSION, Event, Message, Reply

from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted, present
from tests.support.queue import Queue, QueueType


@pytest.mark.parametrize("drained", [False, True])
def test_output_seek_republishes_only_client_queued_capture_storage(
    raw_session: Peer, drained: bool
) -> None:
    peer = raw_session
    output = Queue(peer.video.fd, QueueType.OUTPUT)
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    assert accepted(output.request(1)) == 1
    assert accepted(capture.request(2)) == 2
    accepted(output.stream_on())
    for index in range(2):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    initial = accepted(peer.service.exchange())
    queued = [present(initial.event(index)) for index in range(1, 3)]
    if drained:
        accepted(peer.video.stop())
        assert present(accepted(peer.service.exchange()).event(0)).kind == Event.DRAIN
        done = Message(version=VERSION, kind=Reply.EOS, epoch=1, pool_id=1)
    else:
        done = queued[0].clone()
        done.kind, done.bytesused = Reply.FRAME, 6144
    assert present(accepted(peer.service.exchange([done])).result(0)) == 0
    held = accepted(capture.dequeue())
    assert held.index == 0
    with ThreadPoolExecutor(max_workers=1) as executor:
        stopped = executor.submit(output.stream_off)
        deadline = time.monotonic() + 5
        while True:
            batch = accepted(peer.service.exchange())
            if batch.event_count:
                break
            assert time.monotonic() < deadline
            time.sleep(0.001)
        reset = present(batch.event(0))
        assert reset.kind == Event.RESET and reset.flags == 0 and reset.epoch > 1
        acknowledgement = reset.clone()
        acknowledgement.kind = Reply.STOPPED
        assert (
            present(accepted(peer.service.exchange([acknowledgement])).result(0)) == 0
        )
        accepted(stopped.result(timeout=5))
    # The service destroyed its old decoder and registrations. CAPTURE remains
    # streaming, so the new epoch must reconstruct its authoritative ownership.
    batch = accepted(peer.service.exchange())
    assert batch.event_count == 2
    setup, recycled = present(batch.event(0)), present(batch.event(1))
    assert (setup.kind, setup.epoch, setup.pool_id, setup.count) == (
        Event.CAPTURE,
        reset.epoch,
        1,
        2,
    )
    assert (recycled.kind, recycled.index, recycled.count) == (
        Event.RECYCLE,
        1,
        queued[1].count,
    )
    # A client can return a displayed image while OUTPUT is stopped.
    accepted(capture.queue(held.index))
    returned = present(accepted(peer.service.exchange()).event(0))
    assert returned.kind == Event.RECYCLE and returned.index == held.index
    assert returned.epoch == reset.epoch and returned.count > queued[0].count
    accepted(output.stream_on())
    image = returned.clone()
    image.kind, image.bytesused = Reply.FRAME, 6144
    assert present(accepted(peer.service.exchange([image])).result(0)) == 0
    assert accepted(capture.dequeue()).bytesused == (6144,)


def test_capture_queue_after_streamon_is_visible_before_output_starts(
    raw_session: Peer,
) -> None:
    capture = Queue(raw_session.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(2)) == 2
    accepted(capture.queue(0))
    accepted(capture.stream_on())
    assert accepted(raw_session.service.exchange()).event_count == 2
    accepted(capture.queue(1))
    batch = accepted(raw_session.service.exchange())
    assert batch.event_count == 1
    queued = present(batch.event(0))
    assert queued.kind == Event.RECYCLE and queued.index == 1


def test_output_seek_retires_completed_last_before_client_dequeue(
    raw_session: Peer,
) -> None:
    peer = raw_session
    output, capture = (
        Queue(peer.video.fd, QueueType.OUTPUT),
        Queue(peer.video.fd, QueueType.CAPTURE),
    )
    assert accepted(output.request(1)) == 1
    assert accepted(capture.request(2)) == 2
    accepted(output.stream_on())
    for index in range(2):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    assert accepted(peer.service.exchange()).event_count == 3
    accepted(peer.video.stop())
    assert present(accepted(peer.service.exchange()).event(0)).kind == Event.DRAIN
    end = Message(version=VERSION, kind=Reply.EOS, epoch=1, pool_id=1)
    assert present(accepted(peer.service.exchange([end])).result(0)) == 0
    # Leave the old stream's completed empty LAST queued across the seek.
    with ThreadPoolExecutor(max_workers=1) as executor:
        stopped = executor.submit(output.stream_off)
        deadline = time.monotonic() + 5
        while True:
            batch = accepted(peer.service.exchange())
            if batch.event_count:
                break
            assert time.monotonic() < deadline
            time.sleep(0.001)
        reset = present(batch.event(0))
        assert reset.kind == Event.RESET
        reset.kind = Reply.STOPPED
        assert present(accepted(peer.service.exchange([reset])).result(0)) == 0
        accepted(stopped.result(timeout=5))
    batch = accepted(peer.service.exchange())
    assert batch.event_count == 2
    frame = present(batch.event(1))
    assert frame.kind == Event.RECYCLE and frame.index == 1
    accepted(output.stream_on())
    frame.kind, frame.bytesused = Reply.FRAME, 6144
    assert present(accepted(peer.service.exchange([frame])).result(0)) == 0
    old = accepted(capture.dequeue())
    assert old.bytesused == (0,) and not old.flags & 0x100000
    assert accepted(capture.dequeue()).bytesused == (6144,)


@pytest.mark.parametrize("late", [False, True])
def test_eos_empty_last_republishes_actual_capture_ownership(
    raw_session: Peer, late: bool
) -> None:
    peer = raw_session
    output, capture = (
        Queue(peer.video.fd, QueueType.OUTPUT),
        Queue(peer.video.fd, QueueType.CAPTURE),
    )
    assert accepted(output.request(1)) == 1
    assert accepted(capture.request(2)) == 2
    accepted(output.stream_on())
    for index in range(2):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    initial = accepted(peer.service.exchange())
    tokens = [present(initial.event(index)) for index in (1, 2)]
    if late:
        frames: list[Message] = []
        for token in tokens:
            frame = token.clone()
            frame.kind, frame.bytesused = Reply.FRAME, 6144
            frames.append(frame)
        assert present(accepted(peer.service.exchange(frames)).results(2)) == [0, 0]
        for _ in frames:
            assert accepted(capture.dequeue()).bytesused == (6144,)
    accepted(peer.video.stop())
    assert present(accepted(peer.service.exchange()).event(0)).kind == Event.DRAIN
    end = Message(version=VERSION, kind=Reply.EOS, epoch=1, pool_id=1)
    response = accepted(peer.service.exchange([end]))
    assert present(response.result(0)) == 0
    if late:
        assert response.event_count == 0
        accepted(capture.queue(0))
        response = accepted(peer.service.exchange())
    assert response.event_count == (1 if late else 2)
    snapshot = present(response.event(0))
    assert (snapshot.kind, snapshot.flags, snapshot.count, snapshot.pool_id) == (
        Event.CAPTURE,
        Message.CAPTURE_REUSE,
        2,
        1,
    )
    if not late:
        remaining = present(response.event(1))
        assert (remaining.kind, remaining.index, remaining.count) == (
            Event.RECYCLE,
            1,
            tokens[1].count,
        )
    last = accepted(capture.dequeue())
    assert last.index == 0 and last.flags & 0x100000
