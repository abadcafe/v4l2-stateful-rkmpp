"""module: test_commands; responsibility: specify V4L2 stateful stream control.

operations: pytest cases; entities: none;
tests: drain/resume, normalized try command, unsupported operations and unsubscribe.
"""

import ctypes
import errno
import time
from concurrent.futures import ThreadPoolExecutor

import pytest

from tests.support.bridge_abi import VERSION, Event, Message, Reply
from tests.support.event_abi import DecoderCommand, Subscription
from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import command, ioctl
from tests.support.video import Video


@pytest.mark.parametrize("kind", [0, 1])
def test_decoder_try_valid_command_clears_flags_without_emitting_events(
    raw_session: Peer, kind: int
) -> None:
    payload = DecoderCommand(command=kind)
    raw = bytearray(bytes(payload))
    raw[4:] = b"\xff" * (len(raw) - 4)
    payload = DecoderCommand.from_buffer_copy(raw)
    assert (
        accepted(
            ioctl(
                raw_session.video.fd,
                command(3, "V", 97, ctypes.sizeof(payload)),
                payload,
            )
        )
        == 0
    )
    assert bytes(payload)[4:] == bytes(len(raw) - 4)
    assert accepted(raw_session.service.exchange()).event_count == 0


@pytest.mark.parametrize("kind", [2, 3, 0xFFFFFFFF])
def test_decoder_unsupported_commands_rejected_by_try_and_execute(
    video: Video, kind: int
) -> None:
    payload = DecoderCommand(command=kind)
    for number in (96, 97):
        result = ioctl(
            video.fd, command(3, "V", number, ctypes.sizeof(payload)), payload
        )
        assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_decoder_stop_without_capture_streaming_is_noop(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    peer, _ = packet_session
    accepted(peer.video.stop())
    accepted(peer.video.stop())
    assert accepted(peer.service.exchange()).event_count == 0


@pytest.mark.parametrize("capture_restart", [False, True])
def test_decoder_drain_defers_new_input_until_completed_and_resumed(
    packet_session: tuple[Peer, tuple[Message, ...]],
    capture_restart: bool,
) -> None:
    peer, packets = packet_session
    output = Queue(peer.video.fd, QueueType.OUTPUT)
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    while accepted(peer.service.exchange()).event_count:
        pass
    replies = [Message.from_buffer_copy(message) for message in packets]
    for reply in replies:
        reply.kind = Reply.INPUT_DONE
    assert list(accepted(peer.service.exchange(replies)).results[:3]) == [0, 0, 0]
    assert [accepted(output.dequeue()).index for _ in range(3)] == [0, 1, 2]
    accepted(peer.video.stop())
    batch = accepted(peer.service.exchange())
    drain = Message.from_buffer_copy(batch.events)
    assert batch.event_count == 1 and drain.kind == Event.DRAIN
    for result in (peer.video.stop(), peer.video.start()):
        assert isinstance(result, Rejected) and result.code == errno.EBUSY
    accepted(output.queue(0, 16, 789012))
    assert accepted(peer.service.exchange()).event_count == 0
    eos = Message.from_buffer_copy(drain)
    eos.kind = Reply.EOS
    assert accepted(peer.service.exchange([eos])).results[0] == 0
    result = peer.video.start()
    assert isinstance(result, Rejected) and result.code == errno.EBUSY
    last = accepted(capture.dequeue())
    assert last.flags & 0x100000 and last.bytesused == (0,)
    result = capture.dequeue()
    assert isinstance(result, Rejected) and result.code == errno.EPIPE
    if capture_restart:
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
            assert (
                reset.flags == 9 | (1 << 16) | (1 << 17) and reset.epoch == drain.epoch
            )
            reset.kind = Reply.STOPPED
            assert accepted(peer.service.exchange([reset])).results[0] == 0
            accepted(stopped.result(timeout=5))
        accepted(capture.stream_on())
        accepted(capture.queue(0))
        batch = accepted(peer.service.exchange())
        assert batch.event_count == 4
        assert Message.from_buffer_copy(batch.events).kind == Event.CAPTURE
        offset = ctypes.sizeof(Message)
        first = Message.from_buffer_copy(batch.events, offset)
        second = Message.from_buffer_copy(batch.events, offset + ctypes.sizeof(Message))
        assert first.kind == Event.RESUME and second.kind == Event.PACKET
        assert second.timestamp == 789012000
        return
    accepted(peer.video.start())
    batch = accepted(peer.service.exchange())
    assert batch.event_count == 2
    first = Message.from_buffer_copy(batch.events)
    second = Message.from_buffer_copy(batch.events, ctypes.sizeof(Message))
    assert first.kind == Event.RESUME and second.kind == Event.PACKET
    assert second.timestamp == 789012000


def test_unsubscribe_source_change_stops_public_event_notifications(
    raw_session: Peer,
) -> None:
    accepted(raw_session.video.subscribe(5))
    subscription = Subscription(kind=5)
    assert (
        accepted(
            ioctl(
                raw_session.video.fd,
                command(1, "V", 91, ctypes.sizeof(subscription)),
                subscription,
            )
        )
        == 0
    )
    layout = Message(
        coded_width=128,
        coded_height=64,
        version=VERSION,
        kind=Reply.LAYOUT,
        count=18,
        epoch=1,
        width=128,
        height=64,
        stride=128,
        rows=64,
        size=12288,
        format=1,
    )
    assert accepted(raw_session.service.exchange([layout])).results[0] == 0
    assert accepted(raw_session.video.events()) == []


def test_ioctl_unknown_commands_preserve_sessions(raw_session: Peer) -> None:
    for fd in (raw_session.service.fd, raw_session.video.fd):
        result = ioctl(fd, command(0, "R", 0xFF))
        assert isinstance(result, Rejected) and result.code == errno.ENOTTY
    assert accepted(raw_session.service.exchange()).event_count == 0


def test_decoder_start_before_drain_does_not_reset_stream(raw_session: Peer) -> None:
    accepted(raw_session.video.start())
    assert accepted(raw_session.service.exchange()).event_count == 0


def test_capture_stop_preserves_pending_input_and_epoch(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    peer, packets = packet_session
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    while accepted(peer.service.exchange()).event_count:
        pass
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
        assert reset.kind == Event.RESET
        assert reset.flags == 9 | (1 << 16)
        assert reset.epoch == packets[0].epoch
        reset.kind = Reply.STOPPED
        assert accepted(peer.service.exchange([reset])).results[0] == 0
        accepted(stopped.result(timeout=5))
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    while accepted(peer.service.exchange()).event_count:
        pass
    output = Queue(peer.video.fd, QueueType.OUTPUT)
    for packet in packets:
        done = Message.from_buffer_copy(packet)
        done.kind = Reply.INPUT_DONE
        assert accepted(peer.service.exchange([done])).results[0] == 0
        assert accepted(output.dequeue()).index == packet.index


@pytest.mark.parametrize("stop_requested", [False, True])
@pytest.mark.parametrize("failed_input", [False, True])
def test_natural_eos_excludes_inputs_after_its_sequence_boundary(
    packet_session: tuple[Peer, tuple[Message, ...]],
    stop_requested: bool,
    failed_input: bool,
) -> None:
    peer, packets = packet_session
    output = Queue(peer.video.fd, QueueType.OUTPUT)
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    cap = Message.from_buffer_copy(accepted(peer.service.exchange()).events)
    while accepted(peer.service.exchange()).event_count:
        pass
    if stop_requested:
        accepted(peer.video.stop())
        _ = accepted(peer.service.exchange())
    draining = Message.from_buffer_copy(packets[0])
    draining.kind = Reply.DRAINING
    done = Message.from_buffer_copy(packets[0])
    done.kind = Reply.INPUT_DONE
    done.flags = 2 if failed_input else 0
    eos = Message.from_buffer_copy(cap)
    eos.kind = Reply.EOS
    assert list(accepted(peer.service.exchange([done, draining, eos])).results[:3]) == [
        0,
        0,
        0,
    ]
    assert accepted(capture.dequeue()).flags & 0x100000
    waiting = peer.video.start()
    assert isinstance(waiting, Rejected) and waiting.code == errno.EBUSY
    finished = accepted(output.dequeue())
    assert finished.index == packets[0].index
    assert bool(finished.flags & 0x40) == failed_input
    accepted(peer.video.start())


def test_capture_streamoff_aborts_incomplete_drain(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    peer, _ = packet_session
    capture = Queue(peer.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(18)) == 18
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    while accepted(peer.service.exchange()).event_count:
        pass
    accepted(peer.video.stop())
    _ = accepted(peer.service.exchange())
    with ThreadPoolExecutor(max_workers=1) as executor:
        stopping = executor.submit(capture.stream_off)
        deadline = time.monotonic() + 5
        while True:
            batch = accepted(peer.service.exchange())
            if batch.event_count:
                break
            assert time.monotonic() < deadline
            time.sleep(0.001)
        reset = Message.from_buffer_copy(batch.events)
        assert reset.flags & (1 << 17)
        reset.kind = Reply.STOPPED
        assert accepted(peer.service.exchange([reset])).results[0] == 0
        accepted(stopping.result(timeout=5))
    for index in range(18):
        accepted(capture.queue(index))
    accepted(capture.stream_on())
    accepted(peer.video.start())
    accepted(peer.video.stop())
