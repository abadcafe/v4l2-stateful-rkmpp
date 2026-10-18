"""module: test_exchange_response; responsibility: specify exchange response admission.

operations: pytest cases; entities: HeaderField;
tests: response header, reply cardinality, errno domain and session isolation.
Only the production process's external bridge protocol is exercised.
"""

import ctypes
import errno
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
    ("code", "fatal"),
    [
        (0, False),
        (errno.ESTALE, False),
        (errno.EPIPE, False),
        (errno.EIO, True),
        (4095, True),
    ],
)
def test_exchange_per_reply_errno_tolerates_staleness_and_isolates_fatal_status(
    mock_service: Process,
    code: int,
    fatal: bool,
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
    assert seen == ([Reply.STOPPED, Reply.FAILED] if fatal else [Reply.STOPPED])
    assert mock_service.process.poll() is None


@pytest.mark.parametrize("status", [-2147483648, -4096, 1, 2147483647])
def test_exchange_invalid_reply_status_isolates_session(
    mock_service: Process, status: int
) -> None:
    calls = 0
    seen: list[int] = []

    def handle(request: Request) -> Response:
        nonlocal calls
        calls += 1
        batch = Batch.from_buffer_copy(request.body)
        seen.extend(
            Message.from_buffer_copy(batch.replies, index * ctypes.sizeof(Message)).kind
            for index in range(batch.reply_count)
        )
        if calls == 1:
            batch.event_count = 1
            batch.events[0] = Message(version=VERSION, kind=Event.RESET, epoch=2)
        elif calls == 2:
            assert batch.reply_count == 1
            batch.results[0] = status
            batch.event_count = 1
            batch.events[0] = Message(version=VERSION, kind=Event.CLOSE, epoch=2)
        else:
            return Response(code=errno.EPIPE)
        return Response(body=bytes(batch))

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert seen == [Reply.STOPPED, Reply.FAILED]
    assert "invalid exchange response" in mock_service.log()
    accepted(accepted(mock_service.session(closed)).wait(5))
    assert mock_service.process.poll() is None


@pytest.mark.parametrize("returned", [0, 2, 16])
def test_exchange_response_cannot_change_submitted_reply_count(
    mock_service: Process, returned: int
) -> None:
    calls = 0
    seen: list[int] = []

    def handle(request: Request) -> Response:
        nonlocal calls
        calls += 1
        batch = Batch.from_buffer_copy(request.body)
        seen.extend(
            Message.from_buffer_copy(batch.replies, index * ctypes.sizeof(Message)).kind
            for index in range(batch.reply_count)
        )
        if calls == 1:
            batch.event_count = 1
            batch.events[0] = Message(version=VERSION, kind=Event.RESET, epoch=2)
        elif calls == 2:
            assert batch.reply_count == 1
            batch.reply_count = returned
            batch.event_count = 1
            batch.events[0] = Message(version=VERSION, kind=Event.CLOSE, epoch=2)
        else:
            return Response(code=errno.EPIPE)
        return Response(body=bytes(batch))

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert seen == [Reply.STOPPED, Reply.FAILED]
    assert "invalid exchange response" in mock_service.log()
    accepted(accepted(mock_service.session(closed)).wait(5))
    assert mock_service.process.poll() is None
