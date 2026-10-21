"""module: test_reconfigure; responsibility: specify capture-only source changes.

operations: pytest cases; entities: none;
tests: LAST, retained output epoch, reset acknowledgement and replaced capture ownership.
"""

import errno
import os
import select
import time
from concurrent.futures import ThreadPoolExecutor

import pytest
from rkmpp_bridge_uapi import VERSION, Event, Message, Reply

from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted, present
from tests.support.queue import BufferInfo, Queue, QueueType
from tests.support.video_formats import selection


@pytest.mark.parametrize(
    "initial_layout,start_before_layout",
    [(False, True), (True, True), (False, False)],
    ids=["early-capture", "decoded-capture", "layout-before-streamon"],
)
@pytest.mark.parametrize("size_only", [False, True])
def test_capture_source_change_resets_only_capture_and_keeps_input_valid(
    packet_session: tuple[Peer, tuple[Message, ...]],
    initial_layout: bool,
    start_before_layout: bool,
    size_only: bool,
) -> None:
    peer, packets = packet_session
    accepted(peer.video.subscribe(5))
    first = Message(
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
    if initial_layout:
        assert present(accepted(peer.service.exchange([first])).result(0)) == 0
        assert accepted(peer.video.events()) == [(5, 1)]
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    replacement = first.clone()
    replacement.width = replacement.stride = 64 if size_only else 128
    replacement.coded_width = replacement.width
    replacement.size = 12288
    if not start_before_layout:
        assert present(accepted(peer.service.exchange([replacement])).result(0)) == 0
    accepted(capture.stream_on())
    initial = accepted(peer.service.exchange())
    assert present(initial.event(0)).kind == Event.CAPTURE
    old = present(initial.event(1))
    assert old.kind == Event.RECYCLE
    assert accepted(peer.service.exchange()).event_count == 3
    if start_before_layout:
        assert present(accepted(peer.service.exchange([replacement])).result(0)) == 0
    premature = old.clone()
    premature.kind, premature.index, premature.bytesused = Reply.FRAME, 1, 12288
    premature.generation = 2 if initial_layout else 1
    assert (
        present(accepted(peer.service.exchange([premature])).result(0)) == -errno.ESTALE
    )
    resume = peer.video.start()
    assert isinstance(resume, Rejected) and resume.code == errno.EINVAL
    last = accepted(capture.dequeue())
    assert last.flags & 0x100000 and last.bytesused == (0,)
    # Old-layout recycling is legal until CAPTURE is stopped.
    accepted(capture.queue(last.index))
    assert present(accepted(peer.service.exchange()).event(0)).kind == Event.RECYCLE
    assert accepted(peer.video.events()) == [(5, 1)]
    assert accepted(selection(peer.video.fd, QueueType.CAPTURE)) == (
        replacement.width,
        64,
    )
    with ThreadPoolExecutor(max_workers=1) as executor:
        stopped = executor.submit(capture.stream_off)
        deadline = time.monotonic() + 5
        while True:
            batch = accepted(peer.service.exchange())
            if batch.event_count:
                break
            assert time.monotonic() < deadline
            time.sleep(0.001)
        reset = present(batch.event(0))
        assert reset.kind == Event.RESET and reset.flags == (1 << 16)
        assert reset.epoch == packets[0].epoch
        assert reset.generation == (2 if initial_layout else 1)
        assert not stopped.done()
        reset.kind = Reply.STOPPED
        assert present(accepted(peer.service.exchange([reset])).result(0)) == 0
        accepted(stopped.result(timeout=5))
    # After stop the new layout is authoritative, even without REQBUFS.
    too_small = capture.queue(0)
    assert isinstance(too_small, Rejected) and too_small.code == errno.EINVAL
    assert accepted(capture.request(0)) == 0
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    current = present(accepted(peer.service.exchange()).event(1))
    assert current.kind == Event.RECYCLE and current.pool_id > old.pool_id
    assert accepted(peer.service.exchange()).event_count == 3
    stale = peer.service.register(9, old.epoch, old.pool_id)
    assert isinstance(stale, Rejected) and stale.code == errno.ESTALE
    old.kind, old.bytesused = Reply.FRAME, 6144
    current.kind, current.bytesused = Reply.FRAME, 12288
    completed = packets[0].clone()
    completed.kind = Reply.INPUT_DONE
    statuses = accepted(peer.service.exchange([old, current, completed]))
    assert list(present(statuses.results(3))) == [-errno.ESTALE, 0, 0]
    assert accepted(capture.dequeue()).bytesused == (12288,)
    assert accepted(Queue(peer.video.fd, QueueType.OUTPUT).dequeue()).index == 0
    assert Message.SIZE == 120


@pytest.mark.parametrize("draining", [False, True])
@pytest.mark.parametrize("dequeue_last", [False, True])
def test_capture_source_change_reuses_sufficient_pool_without_streamoff(
    packet_session: tuple[Peer, tuple[Message, ...]], draining: bool, dequeue_last: bool
) -> None:
    peer, packets = packet_session
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    old = present(accepted(peer.service.exchange()).event(0))
    while accepted(peer.service.exchange()).event_count:
        pass
    if draining:
        accepted(peer.video.stop())
        assert present(accepted(peer.service.exchange()).event(0)).kind == Event.DRAIN
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
    assert present(accepted(peer.service.exchange([layout])).result(0)) == 0
    if dequeue_last:
        last = accepted(capture.dequeue())
        assert last.flags & 0x100000
        accepted(capture.queue(last.index))
    while accepted(peer.service.exchange()).event_count:
        pass
    accepted(peer.video.start())
    events: list[Message] = []
    while True:
        batch = accepted(peer.service.exchange())
        if batch.event_count == 0:
            break
        events.extend(present(batch.event(i)) for i in range(batch.event_count))
    assert events[0].kind == Event.CAPTURE
    assert events[0].pool_id == old.pool_id
    assert events[0].generation == 1
    assert [event.index for event in events[1:]] == list(range(18))
    frame = events[1].clone()
    frame.kind, frame.bytesused = Reply.FRAME, 6144
    assert present(accepted(peer.service.exchange([frame])).result(0)) == 0
    decoded = accepted(capture.dequeue())
    assert decoded.bytesused == (6144,)
    assert not decoded.flags & 0x100000
    if draining:
        result = peer.video.stop()
        assert isinstance(result, Rejected) and result.code == errno.EBUSY


@pytest.mark.parametrize("blocking_dequeue", [False, True])
def test_source_change_wakes_waiting_capture_client(
    packet_session: tuple[Peer, tuple[Message, ...]], blocking_dequeue: bool
) -> None:
    peer, packets = packet_session
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    while accepted(peer.service.exchange()).event_count:
        pass
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

    def wait_capture() -> BufferInfo:
        if not blocking_dequeue:
            assert peer.video.poll(2000) & select.POLLIN
        return accepted(capture.dequeue())

    os.set_blocking(peer.video.fd, blocking_dequeue)
    try:
        with ThreadPoolExecutor(max_workers=1) as executor:
            waiting = executor.submit(wait_capture)
            time.sleep(0.02)
            was_waiting = not waiting.done()
            assert present(accepted(peer.service.exchange([layout])).result(0)) == 0
            last = waiting.result(timeout=5)
            assert was_waiting
            assert last.flags & 0x100000 and last.bytesused == (0,)
    finally:
        os.set_blocking(peer.video.fd, False)
