"""module: test_reconfigure; responsibility: specify capture-only source changes.

operations: pytest cases; entities: none;
tests: LAST, retained output epoch, reset acknowledgement and replaced capture ownership.
"""

import ctypes
import errno
import time
from concurrent.futures import ThreadPoolExecutor

from tests.support.bridge_abi import VERSION, Event, Message, Reply
from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType
from tests.support.video_formats import selection


def test_capture_source_change_resets_only_capture_and_keeps_input_valid(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    peer, packets = packet_session
    accepted(peer.video.subscribe(5))
    first = Message(
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
    assert accepted(peer.service.exchange([first])).results[0] == 0
    assert accepted(peer.video.events()) == [(5, 1)]
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    old = Message.from_buffer_copy(accepted(peer.service.exchange()).events)
    assert old.kind == Event.CAPTURE
    assert accepted(peer.service.exchange()).event_count == 3
    replacement = Message.from_buffer_copy(first)
    replacement.width = replacement.stride = 128
    replacement.size = 12288
    assert accepted(peer.service.exchange([replacement])).results[0] == 0
    last = accepted(capture.dequeue())
    assert last.flags & 0x100000 and last.bytesused == (0,)
    assert accepted(peer.video.events()) == [(5, 1)]
    assert accepted(selection(peer.video.fd, QueueType.CAPTURE)) == (128, 64)
    with ThreadPoolExecutor(max_workers=1) as executor:
        stopped = executor.submit(capture.stream_off)
        deadline = time.monotonic() + 5
        while True:
            batch = accepted(peer.service.exchange())
            if batch.event_count:
                break
            assert time.monotonic() < deadline
            time.sleep(0.001)
        reset = Message.from_buffer_copy(batch.events)
        assert reset.kind == Event.RESET and reset.flags == 9 | (1 << 16)
        assert reset.epoch == packets[0].epoch and reset.generation > old.generation
        assert not stopped.done()
        reset.kind = Reply.STOPPED
        assert accepted(peer.service.exchange([reset])).results[0] == 0
        accepted(stopped.result(timeout=5))
    assert accepted(capture.request(0)) == 0
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    current = Message.from_buffer_copy(accepted(peer.service.exchange()).events)
    assert current.kind == Event.CAPTURE and current.pool_id > old.pool_id
    assert accepted(peer.service.exchange()).event_count == 3
    stale = peer.service.register(9, old.epoch, old.pool_id)
    assert isinstance(stale, Rejected) and stale.code == errno.ESTALE
    old.kind, old.bytesused = Reply.FRAME, 6144
    current.kind, current.bytesused = Reply.FRAME, 12288
    completed = Message.from_buffer_copy(packets[0])
    completed.kind = Reply.INPUT_DONE
    statuses = accepted(peer.service.exchange([old, current, completed]))
    assert list(statuses.results[:3]) == [-errno.ESTALE, 0, 0]
    assert accepted(capture.dequeue()).bytesused == (12288,)
    assert accepted(Queue(peer.video.fd, QueueType.OUTPUT).dequeue()).index == 0
    assert ctypes.sizeof(current) == 104
