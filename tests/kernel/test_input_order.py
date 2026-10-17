"""module: test_input_order; responsibility: specify compressed input ordering.

operations: pytest cases; entities: none; tests: prequeued FIFO and payload range.
"""

import ctypes
import time
from concurrent.futures import ThreadPoolExecutor

from tests.support.bridge_abi import Event, Message, Reply
from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted
from tests.support.queue import Queue, QueueType


def test_streamon_preserves_qbuf_order_and_payload_offset(raw_session: Peer) -> None:
    output = Queue(raw_session.video.fd, QueueType.OUTPUT)
    assert accepted(output.request(3)) == 3
    for index in (2, 0, 1):
        accepted(output.queue(index, 32, 100 + index, data_offset=16))
    accepted(output.stream_on())
    batch = accepted(raw_session.service.exchange())
    events = [
        Message.from_buffer_copy(batch.events, index * ctypes.sizeof(Message))
        for index in range(batch.event_count)
    ]
    assert [event.index for event in events] == [2, 0, 1]
    assert all(event.kind == Event.PACKET for event in events)
    assert [(event.data_offset, event.bytesused) for event in events] == [(16, 16)] * 3


def test_output_sequence_counts_submissions_independently_of_slot_reuse(
    raw_session: Peer,
) -> None:
    output = Queue(raw_session.video.fd, QueueType.OUTPUT)
    assert accepted(output.request(3)) == 3
    accepted(output.stream_on())
    for sequence, index in enumerate((2, 0, 1, 1, 2, 0)):
        accepted(output.queue(index, 16))
        packet = Message.from_buffer_copy(
            accepted(raw_session.service.exchange()).events
        )
        packet.kind = Reply.INPUT_DONE
        assert accepted(raw_session.service.exchange([packet])).results[0] == 0
        done = accepted(output.dequeue())
        assert (done.index, done.sequence) == (index, sequence)
    with ThreadPoolExecutor(max_workers=1) as executor:
        stopped = executor.submit(output.stream_off)
        deadline = time.monotonic() + 5
        while True:
            batch = accepted(raw_session.service.exchange())
            if batch.event_count:
                break
            assert time.monotonic() < deadline
            time.sleep(0.001)
        reset = Message.from_buffer_copy(batch.events)
        assert reset.kind == Event.RESET
        reset.kind = Reply.STOPPED
        assert accepted(raw_session.service.exchange([reset])).results[0] == 0
        accepted(stopped.result(timeout=5))
    accepted(output.stream_on())
    accepted(output.queue(2, 16))
    packet = Message.from_buffer_copy(accepted(raw_session.service.exchange()).events)
    packet.kind = Reply.INPUT_DONE
    assert accepted(raw_session.service.exchange([packet])).results[0] == 0
    done = accepted(output.dequeue())
    assert (done.index, done.sequence) == (2, 0)
