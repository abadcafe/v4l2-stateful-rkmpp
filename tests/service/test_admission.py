"""module: test_admission; responsibility: specify daemon admission fault isolation.

operations: process acceptance cases; entities: none;
tests: rejected offers preserve live sessions, pressure backs off, fatal errors exit.
"""

import errno
import threading
import time
from concurrent.futures import ThreadPoolExecutor

import pytest
from rkmpp_bridge_uapi import EXCHANGE, Batch

from tests.support.mock_process import Process
from tests.support.mock_wire import Reply, Request
from tests.support.outcome import accepted, present


def idle(request: Request) -> Reply:
    assert request.command == EXCHANGE
    return Reply(body=bytes(present(Batch.decode(request.body))))


@pytest.mark.parametrize(
    "code", [errno.EPIPE, errno.ENOMEM, errno.EMFILE, errno.ENFILE]
)
def test_rejected_offer_preserves_existing_session_and_accepts_next(
    mock_service: Process, code: int
) -> None:
    observed = threading.Event()

    def handle(request: Request) -> Reply:
        assert request.command == EXCHANGE
        observed.set()
        return Reply(body=bytes(present(Batch.decode(request.body))))

    first = accepted(mock_service.session(handle))
    assert observed.wait(5)
    accepted(mock_service.reject_accept(code))
    second = accepted(mock_service.session(idle))
    assert mock_service.process.poll() is None
    observed.clear()
    accepted(first.wake())
    assert observed.wait(5)
    accepted(second.wake())
    assert mock_service.process.poll() is None


def test_repeated_pressure_is_rate_limited_and_shutdown_remains_responsive(
    mock_service: Process,
) -> None:
    for _ in range(30):
        accepted(mock_service.reject_accept(errno.ENOMEM))
    time.sleep(0.35)
    count = mock_service.log().count("admission pressure")
    assert 1 <= count <= 5
    began = time.monotonic()
    mock_service.process.terminate()
    assert mock_service.process.wait(timeout=2) == 0
    assert time.monotonic() - began < 1


def test_sessions_do_not_create_native_threads(mock_service: Process) -> None:
    observed = threading.Event()

    def handle(request: Request) -> Reply:
        assert request.command == EXCHANGE
        observed.set()
        return Reply(body=bytes(present(Batch.decode(request.body))))

    first = accepted(mock_service.session(handle))
    assert observed.wait(5)
    # Hardware executor already exists. Reject all subsequent native thread
    # creation: async admission and scheduling must still operate normally.
    mock_service.thread_failure.touch()
    observed.clear()
    second = accepted(mock_service.session(handle))
    assert observed.wait(5)
    assert mock_service.process.poll() is None
    observed.clear()
    accepted(first.wake())
    assert observed.wait(5)
    accepted(second.wake())
    assert mock_service.process.poll() is None


def test_session_admission_waits_for_capacity_and_resumes_after_close(
    mock_service: Process,
) -> None:
    # Bound the whole session lifetime, including tasks waiting for an
    # executor lane. A bounded lane queue alone cannot limit these owners.
    peers = [accepted(mock_service.session(idle)) for _ in range(64)]
    with ThreadPoolExecutor(max_workers=1) as executor:
        pending = executor.submit(mock_service.session, idle)
        try:
            time.sleep(0.2)
            assert not pending.done(), "session admission exceeded its capacity"
        finally:
            peers[0].close()
        next_peer = accepted(pending.result(timeout=5))
        accepted(next_peer.wake())
    assert mock_service.process.poll() is None
