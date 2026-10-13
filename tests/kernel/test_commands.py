"""module: test_commands; responsibility: specify V4L2 stateful stream control.

operations: pytest cases; entities: none;
tests: drain/resume, normalized try command, unsupported operations and unsubscribe.
"""

import ctypes
import errno

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


def test_decoder_stop_is_idempotent_resume_reopens_output_queue(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    peer, packets = packet_session
    output = Queue(peer.video.fd, QueueType.OUTPUT)
    replies = [Message.from_buffer_copy(message) for message in packets]
    for reply in replies:
        reply.kind = Reply.INPUT_DONE
    assert list(accepted(peer.service.exchange(replies)).results[:3]) == [0, 0, 0]
    assert [accepted(output.dequeue()).index for _ in range(3)] == [0, 1, 2]
    accepted(peer.video.stop())
    accepted(peer.video.stop())
    batch = accepted(peer.service.exchange())
    assert (
        batch.event_count == 1
        and Message.from_buffer_copy(batch.events).kind == Event.DRAIN
    )
    result = output.queue(0, 16)
    assert isinstance(result, Rejected) and result.code == errno.EPIPE
    accepted(peer.video.start())
    accepted(output.queue(0, 16, 789012))
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
        version=VERSION,
        kind=Reply.LAYOUT,
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
