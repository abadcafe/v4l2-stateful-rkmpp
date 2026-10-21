"""module: test_transport_isolation; responsibility: specify blocking ioctl isolation.

operations: pytest cases; entities: none; tests: a stalled exchange preserves
diagnostic and unrelated session progress through the real service process.
"""

import threading

from rkmpp_bridge_uapi import EXCHANGE, VERSION, Batch, Event, Message

from tests.support.diagnostics import query
from tests.support.mock_process import Process
from tests.support.mock_wire import Reply, Request
from tests.support.outcome import accepted


def test_exchange_blocked_peer_preserves_diagnostics_and_other_session_progress(
    mock_service: Process,
) -> None:
    started = threading.Event()
    release = threading.Event()

    def blocked(request: Request) -> Reply:
        assert request.command == EXCHANGE
        started.set()
        assert release.wait(10)
        batch = Batch(version=VERSION, event_count=1)
        assert batch.set_event(0, Message(version=VERSION, kind=Event.CLOSE))
        return Reply(body=bytes(batch))

    first = accepted(mock_service.session(blocked))
    try:
        assert started.wait(5)
        assert accepted(query(mock_service.diagnostics)).state == "ready"

        def closed(request: Request) -> Reply:
            assert request.command == EXCHANGE
            batch = Batch(version=VERSION, event_count=1)
            assert batch.set_event(0, Message(version=VERSION, kind=Event.CLOSE))
            return Reply(body=bytes(batch))

        accepted(accepted(mock_service.session(closed)).wait(5))
        assert mock_service.process.poll() is None
    finally:
        release.set()
    accepted(first.wait(5))
