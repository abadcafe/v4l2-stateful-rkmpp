"""module: test_protocol; responsibility: specify daemon exchange progression.

operations: pytest cases; entities: batch/event scenario names;
tests: errno, idle wake, batch continuation, EOS, reset and broker retry.
Every call reaches the unmodified production ELF through its kernel-facing ABI.
"""

import errno
import threading
import time

import pytest
from rkmpp_bridge_uapi import EXCHANGE, VERSION, Batch, Event, Message, Reply

from tests.support.mock_process import Process
from tests.support.mock_wire import Reply as Response
from tests.support.mock_wire import Request
from tests.support.outcome import accepted, present


def closed(request: Request) -> Response:
    assert request.command == EXCHANGE
    batch = present(Batch.decode(request.body))
    batch.event_count = 1
    assert batch.set_event(0, Message(version=VERSION, kind=Event.CLOSE, epoch=1))
    return Response(body=bytes(batch))


@pytest.mark.parametrize("code", [errno.EIO, errno.EINTR, errno.EAGAIN, errno.EPIPE])
def test_exchange_kernel_errno_has_bounded_session_exit(
    mock_service: Process,
    code: int,
) -> None:
    seen: list[int] = []

    def handle(request: Request) -> Response:
        batch = present(Batch.decode(request.body))
        seen.extend(present(batch.reply(i)).kind for i in range(batch.reply_count))
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


def test_exchange_full_event_batch_fetches_next_batch_without_poll_wake(
    mock_service: Process,
) -> None:
    calls = 0

    def handle(request: Request) -> Response:
        nonlocal calls
        calls += 1
        if calls == 2:
            return closed(request)
        batch = present(Batch.decode(request.body))
        batch.event_count = 16
        for index in range(16):
            assert batch.set_event(
                index, Message(version=VERSION, kind=Event.RESUME, epoch=1)
            )
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
        batch = present(Batch.decode(request.body))
        if first:
            first = False
            batch.event_count = 1
            assert batch.set_event(
                0, Message(version=VERSION, kind=Event.DRAIN, epoch=1)
            )
            return Response(body=bytes(batch))
        replies.extend(present(batch.reply(i)).kind for i in range(batch.reply_count))
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
        batch = present(Batch.decode(request.body))
        if not sent:
            sent = True
            batch.event_count = 1
            assert batch.set_event(
                0,
                Message(
                    version=VERSION, kind=Event.RESET, count=1, flags=1 << 16, epoch=1
                ),
            )
            return Response(body=bytes(batch))
        replies.extend(
            present(batch.reply(index)).kind for index in range(batch.reply_count)
        )
        return Response(code=errno.EPIPE)

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert replies == [Reply.STOPPED]
    assert mock_service.process.poll() is None
