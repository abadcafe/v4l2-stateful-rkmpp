"""module: test_protocol; responsibility: specify daemon exchange failure isolation.

operations: pytest cases; entities: batch/event scenario names;
tests: malformed exchanges, unsupported events, errno, idle wake and broker retry.
Every call reaches the unmodified production ELF through its kernel-facing ABI.
"""

import ctypes
import errno
import threading
import time
from typing import Literal

import pytest

from tests.support.bridge_abi import EXCHANGE, VERSION, Batch, Event, Message, Reply
from tests.support.mock_process import Process
from tests.support.mock_wire import Reply as Response
from tests.support.mock_wire import Request
from tests.support.outcome import accepted

type HeaderField = Literal["version", "reply_count", "event_count", "reserved"]


def closed(request: Request) -> Response:
    assert request.command == EXCHANGE
    batch = Batch.from_buffer_copy(request.body)
    batch.event_count = 1
    batch.events[0] = Message(version=VERSION, kind=Event.CLOSE, epoch=1)
    return Response(body=bytes(batch))


@pytest.mark.parametrize("field", ["version", "reply_count", "event_count", "reserved"])
def test_exchange_invalid_response_header_reports_failed_and_keeps_broker_alive(
    mock_service: Process,
    field: HeaderField,
) -> None:
    failed: list[Message] = []

    def handle(request: Request) -> Response:
        assert request.command == EXCHANGE
        batch = Batch.from_buffer_copy(request.body)
        if batch.reply_count:
            failed.extend(
                Message.from_buffer_copy(batch.replies, i * ctypes.sizeof(Message))
                for i in range(batch.reply_count)
            )
            return Response(code=errno.EPIPE)
        if field == "version":
            batch.version += 1
        elif field == "reserved":
            batch.reserved = 1
        elif field == "reply_count":
            batch.reply_count = 17
        else:
            batch.event_count = 17
        return Response(body=bytes(batch))

    peer = accepted(mock_service.session(handle))
    accepted(peer.wait(5))
    assert [message.kind for message in failed] == [Reply.FAILED]
    assert "invalid exchange header" in mock_service.log()
    accepted(accepted(mock_service.session(closed)).wait(5))
    assert mock_service.process.poll() is None


@pytest.mark.parametrize(
    ("kind", "version", "flags"),
    [
        (999, VERSION, 0),
        (Event.PACKET, VERSION, 0),
        (Event.RECYCLE, VERSION, 0),
        (Event.CLOSE, VERSION + 1, 0),
    ],
    ids=[
        "unknown",
        "zero-packet-pool",
        "zero-recycle-pool",
        "event-version",
    ],
)
def test_exchange_invalid_event_returns_failed_without_killing_other_sessions(
    mock_service: Process,
    kind: int,
    version: int,
    flags: int,
) -> None:
    first = True
    replies: list[int] = []

    def handle(request: Request) -> Response:
        nonlocal first
        assert request.command == EXCHANGE
        batch = Batch.from_buffer_copy(request.body)
        if first:
            first = False
            batch.event_count = 1
            batch.events[0] = Message(
                version=version, kind=kind, flags=flags, epoch=1, count=1
            )
            return Response(body=bytes(batch))
        replies.extend(
            Message.from_buffer_copy(batch.replies, i * ctypes.sizeof(Message)).kind
            for i in range(batch.reply_count)
        )
        return Response(code=errno.EPIPE)

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert replies == [Reply.FAILED]
    accepted(accepted(mock_service.session(closed)).wait(5))
    assert mock_service.process.poll() is None


@pytest.mark.parametrize("code", [errno.EIO, errno.EINTR, errno.EAGAIN, errno.EPIPE])
def test_exchange_kernel_errno_has_bounded_session_exit(
    mock_service: Process,
    code: int,
) -> None:
    seen: list[int] = []

    def handle(request: Request) -> Response:
        batch = Batch.from_buffer_copy(request.body)
        seen.extend(
            Message.from_buffer_copy(batch.replies, i * ctypes.sizeof(Message)).kind
            for i in range(batch.reply_count)
        )
        return Response(code=code)

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert seen == ([] if code == errno.EPIPE else [Reply.FAILED])
    assert mock_service.process.poll() is None


@pytest.mark.parametrize("code", [errno.EINTR, errno.EAGAIN])
def test_accept_retryable_errno_allows_the_next_session(
    mock_service: Process,
    code: int,
) -> None:
    accepted(mock_service.reject_accept(code))
    accepted(accepted(mock_service.session(closed)).wait(5))
    assert mock_service.process.poll() is None


def test_exchange_idle_session_sleeps_and_real_socket_poll_wakes_it(
    mock_service: Process,
) -> None:
    idle = threading.Event()
    calls = 0

    def handle(request: Request) -> Response:
        nonlocal calls
        assert request.command == EXCHANGE
        calls += 1
        if calls == 1:
            idle.set()
            return Response(body=bytes(Batch(version=VERSION)))
        return closed(request)

    peer = accepted(mock_service.session(handle))
    assert idle.wait(5)
    time.sleep(0.05)
    assert len(peer.calls()) == 1
    accepted(peer.wake())
    accepted(peer.wait(5))
    assert len(peer.calls()) == 2


@pytest.mark.parametrize("code", [errno.ESTALE, errno.EPIPE, errno.EIO])
def test_exchange_per_reply_errno_tolerates_staleness_and_isolates_fatal_status(
    mock_service: Process,
    code: int,
) -> None:
    calls = 0
    seen: list[int] = []

    def handle(request: Request) -> Response:
        nonlocal calls
        assert request.command == EXCHANGE
        calls += 1
        batch = Batch.from_buffer_copy(request.body)
        seen.extend(
            Message.from_buffer_copy(batch.replies, i * ctypes.sizeof(Message)).kind
            for i in range(batch.reply_count)
        )
        if calls == 1:
            batch.event_count = 1
            batch.events[0] = Message(version=VERSION, kind=Event.RESET, epoch=2)
        elif calls == 2:
            assert batch.reply_count == 1 and seen == [Reply.STOPPED]
            batch.results[0] = -code
            batch.event_count = 1
            batch.events[0] = Message(version=VERSION, kind=Event.CLOSE, epoch=2)
        else:
            return Response(code=errno.EPIPE)
        return Response(body=bytes(batch))

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert seen == (
        [Reply.STOPPED, Reply.FAILED] if code == errno.EIO else [Reply.STOPPED]
    )
    assert mock_service.process.poll() is None


def test_exchange_full_event_batch_fetches_next_batch_without_poll_wake(
    mock_service: Process,
) -> None:
    calls = 0

    def handle(request: Request) -> Response:
        nonlocal calls
        calls += 1
        if calls == 2:
            return closed(request)
        batch = Batch.from_buffer_copy(request.body)
        batch.event_count = 16
        for index in range(16):
            batch.events[index] = Message(version=VERSION, kind=Event.RESUME, epoch=1)
        return Response(body=bytes(batch))

    peer = accepted(mock_service.session(handle))
    accepted(peer.wait(5))
    assert len(peer.calls()) == 2
    assert "max_events=16" in mock_service.log()


def test_accept_fatal_errno_exits_broker_with_failure(mock_service: Process) -> None:
    accepted(mock_service.reject_accept(errno.EIO))
    assert mock_service.process.wait(timeout=5) == 1
    assert "accept control" in mock_service.log()


def test_exchange_empty_drain_completes_without_initializing_mpp(
    mock_service: Process,
) -> None:
    first = True
    replies: list[int] = []

    def handle(request: Request) -> Response:
        nonlocal first
        assert request.command == EXCHANGE
        batch = Batch.from_buffer_copy(request.body)
        if first:
            first = False
            batch.event_count = 1
            batch.events[0] = Message(version=VERSION, kind=Event.DRAIN, epoch=1)
            return Response(body=bytes(batch))
        replies.extend(
            Message.from_buffer_copy(batch.replies, i * ctypes.sizeof(Message)).kind
            for i in range(batch.reply_count)
        )
        if replies:
            return closed(request)
        return Response(body=bytes(batch))

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert replies == [Reply.EOS]


def test_exchange_capture_reset_before_decoder_initialization_completes(
    mock_service: Process,
) -> None:
    sent = False
    replies: list[int] = []

    def handle(request: Request) -> Response:
        nonlocal sent
        assert request.command == EXCHANGE
        batch = Batch.from_buffer_copy(request.body)
        if not sent:
            sent = True
            batch.event_count = 1
            batch.events[0] = Message(
                version=VERSION, kind=Event.RESET, flags=1 << 16, epoch=1
            )
            return Response(body=bytes(batch))
        replies.extend(
            Message.from_buffer_copy(batch.replies, index * ctypes.sizeof(Message)).kind
            for index in range(batch.reply_count)
        )
        return Response(code=errno.EPIPE)

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert replies == [Reply.STOPPED]
    assert mock_service.process.poll() is None


@pytest.mark.parametrize("kind", list(Event))
@pytest.mark.parametrize("reserved", [1, 0xFFFFFFFF])
def test_event_reserved_is_rejected_before_resource_transfer_or_state_mutation(
    mock_service: Process, kind: Event, reserved: int
) -> None:
    first = True
    responses: list[Message] = []

    def handle(request: Request) -> Response:
        nonlocal first
        # Rejection must precede REGISTER_POOL, CLOSE, or backend RESET.
        assert request.command == EXCHANGE
        batch = Batch.from_buffer_copy(request.body)
        if first:
            first = False
            batch.event_count = 1
            message = Message(
                version=VERSION, kind=kind, epoch=2, generation=3, pool_id=7
            )
            message.reserved[0] = reserved
            batch.events[0] = message
            return Response(body=bytes(batch))
        responses.extend(
            Message.from_buffer_copy(batch.replies, i * ctypes.sizeof(Message))
            for i in range(batch.reply_count)
        )
        return Response(code=errno.EPIPE)

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert len(responses) == 1
    failed = responses[0]
    assert (failed.kind, failed.epoch, failed.generation) == (Reply.FAILED, 2, 3)
    assert list(failed.reserved) == [0]
    accepted(accepted(mock_service.session(closed)).wait(5))
    assert mock_service.process.poll() is None
