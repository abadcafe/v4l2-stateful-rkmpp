"""module: cdp.transport_tests; responsibility: specify owned WebSocket I/O.

operations: tests; entities: none; tests: exchange, deadline recovery,
background protocol progress, connection failures and complete shutdown.
"""

import asyncio
import errno
import math
import threading
import time

import pytest
from websockets.sync.server import ServerConnection

from tests.conftest import PeerFactory
from tests.support.cdp.transport import Transport
from tests.support.outcome import Rejected, accepted


def test_open_exchange_and_close_join_the_network_owner(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        assert connection.recv() == "request"
        connection.send("response")

    url = peer(handle)
    baseline = {thread.ident for thread in threading.enumerate()}
    transport = accepted(Transport.open(url))
    try:
        accepted(transport.send("request", 1))
        assert accepted(transport.receive(1)) == "response"
    finally:
        transport.close()
    transport.close()
    deadline = time.monotonic() + 1
    while {thread.ident for thread in threading.enumerate()} - baseline:
        assert time.monotonic() < deadline
        _ = threading.Event().wait(0.01)


def test_receive_timeout_preserves_the_next_message(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        assert connection.recv() == "release"
        connection.send("retained")

    transport = accepted(Transport.open(peer(handle)))
    try:
        result = transport.receive(0.01)
        assert isinstance(result, Rejected) and result.code == errno.ETIMEDOUT
        accepted(transport.send("release", 1))
        assert accepted(transport.receive(1)) == "retained"
    finally:
        transport.close()


def test_open_invalid_uri_releases_the_network_thread() -> None:
    result = Transport.open("not-a-websocket")
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_open_event_loop_resource_failure_returns_without_waiting_for_a_socket(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def exhausted(_runner: asyncio.Runner) -> asyncio.AbstractEventLoop:
        raise OSError(errno.EMFILE, "event loop descriptor allocation")

    monkeypatch.setattr(asyncio.Runner, "get_loop", exhausted)
    result = Transport.open("ws://127.0.0.1:1")
    assert isinstance(result, Rejected) and result.code == errno.EMFILE


@pytest.mark.parametrize("timeout", [0, -1, math.nan, math.inf, -math.inf])
def test_send_receive_invalid_deadlines_leave_connection_usable(
    peer: PeerFactory, timeout: float
) -> None:
    def handle(connection: ServerConnection) -> None:
        assert connection.recv() == "valid"
        connection.send("reply")

    transport = accepted(Transport.open(peer(handle)))
    try:
        for result in (transport.send("invalid", timeout), transport.receive(timeout)):
            assert isinstance(result, Rejected) and result.code == errno.EINVAL
        accepted(transport.send("valid", 1))
        assert accepted(transport.receive(1)) == "reply"
    finally:
        transport.close()


def test_close_rejects_new_io_without_leaking_coroutines(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        assert tuple(connection) == ()

    transport = accepted(Transport.open(peer(handle)))
    transport.close()
    for result in (transport.send("closed", 1), transport.receive(1)):
        assert isinstance(result, Rejected) and result.code == errno.ECONNRESET
