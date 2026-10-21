"""module: test_stop_serialization; responsibility: specify stop transaction isolation.

operations: pytest cases; entities: none; tests: concurrent commands wait for reset.
"""

import errno
import time
from concurrent.futures import ThreadPoolExecutor

import pytest
from rkmpp_bridge_uapi import Event, Message, Reply

from tests.support.event_abi import DecoderCommand
from tests.support.kernel_peer import Peer
from tests.support.outcome import Ok, accepted, present
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import command, ioctl


@pytest.mark.parametrize("operation", ["streamoff", "start", "stop"])
def test_stop_serializes_other_mutating_operations(
    packet_session: tuple[Peer, tuple[Message, ...]], operation: str
) -> None:
    peer, _ = packet_session
    output = Queue(peer.video.fd, QueueType.OUTPUT)
    with ThreadPoolExecutor(max_workers=2) as executor:
        first = executor.submit(output.stream_off)
        deadline = time.monotonic() + 3
        while True:
            batch = accepted(peer.service.exchange())
            if batch.event_count:
                break
            assert time.monotonic() < deadline
            time.sleep(0.001)
        reset = present(batch.event(0))
        assert reset.kind == Event.RESET
        if operation == "streamoff":
            second = executor.submit(output.stream_off)
        else:
            payload = DecoderCommand(command=0 if operation == "start" else 1)
            second = executor.submit(
                ioctl, peer.video.fd, command(3, "V", 96, 72), payload
            )
        time.sleep(0.05)
        waited = not second.done()
        acknowledgement = reset.clone()
        acknowledgement.kind = Reply.STOPPED
        assert (
            present(accepted(peer.service.exchange([acknowledgement])).result(0)) == 0
        )
        accepted(first.result(timeout=3))
        assert isinstance(second.result(timeout=3), Ok)
        assert waited


def test_capture_stop_accepts_already_inflight_completions_before_acknowledgement(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    peer, packets = packet_session
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    _ = accepted(capture.request(18))
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    cap = present(accepted(peer.service.exchange()).event(1))
    assert cap.kind == Event.RECYCLE
    while accepted(peer.service.exchange()).event_count:
        pass
    with ThreadPoolExecutor(max_workers=1) as executor:
        stopping = executor.submit(capture.stream_off)
        deadline = time.monotonic() + 3
        while True:
            batch = accepted(peer.service.exchange())
            if batch.event_count:
                break
            assert time.monotonic() < deadline
            time.sleep(0.001)
        reset = present(batch.event(0))
        assert reset.kind == Event.RESET and reset.epoch == packets[0].epoch
        done = packets[0].clone()
        done.kind = Reply.INPUT_DONE
        frame = cap.clone()
        frame.kind = Reply.FRAME
        frame.bytesused = 6144
        reset.kind = Reply.STOPPED
        results = present(
            accepted(peer.service.exchange([done, frame, reset])).results(3)
        )
        accepted(stopping.result(timeout=3))
        assert list(results[:3]) == [0, 0, 0]
    # OUTPUT remains live and its completion is not lost during CAPTURE teardown.
    assert accepted(Queue(peer.video.fd, QueueType.OUTPUT).dequeue()).index == 0
    assert accepted(capture.query(0)).flags & 0x4 == 0


def test_capture_stop_rejects_acknowledgement_from_previous_stop(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    peer, _ = packet_session
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    _ = accepted(capture.request(18))
    previous: Message | None = None
    with ThreadPoolExecutor(max_workers=1) as executor:
        for _ in range(2):
            for index in range(18):
                accepted(capture.queue(index))
            accepted(capture.stream_on())
            while accepted(peer.service.exchange()).event_count:
                pass
            stopping = executor.submit(capture.stream_off)
            deadline = time.monotonic() + 3
            while True:
                batch = accepted(peer.service.exchange())
                if batch.event_count:
                    break
                assert time.monotonic() < deadline
                time.sleep(0.001)
            reset = present(batch.event(0))
            assert reset.kind == Event.RESET
            assert reset.count != 0
            reset.kind = Reply.STOPPED
            if previous is not None:
                assert (reset.epoch, reset.generation) == (
                    previous.epoch,
                    previous.generation,
                )
                stale = accepted(peer.service.exchange([previous]))
                # Complete the actual stop even when this regression fails.
                current = accepted(peer.service.exchange([reset]))
                accepted(stopping.result(timeout=3))
                assert present(stale.result(0)) == -errno.ESTALE
                assert present(current.result(0)) == 0
            else:
                zero = reset.clone()
                zero.count = 0
                assert (
                    present(accepted(peer.service.exchange([zero])).result(0))
                    == -errno.ESTALE
                )
                assert present(accepted(peer.service.exchange([reset])).result(0)) == 0
                accepted(stopping.result(timeout=3))
            previous = reset
