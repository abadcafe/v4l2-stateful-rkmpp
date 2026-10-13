"""module: diagnostics_tests; responsibility: define the read-only status protocol.

operations: pytest cases; entities: none; tests: query, peer credentials, limits.
The substituted boundary is an actual local Unix socket server.
"""

import errno
import json
import os
import socket
import threading
from collections.abc import Generator
from contextlib import contextmanager
from pathlib import Path

import pytest

from tests.support.diagnostics import query
from tests.support.outcome import Rejected, accepted


def _status() -> dict[str, object]:
    return {
        "protocol": 1,
        "identity": {
            "pid": os.getpid(),
            "uid": os.geteuid(),
            "start_ticks": 1,
            "build_id": "0123456789abcdef",
            "control": {"device": 1, "inode": 2, "raw_device": 3},
        },
        "state": "ready",
        "cpu_seconds": 0.25,
        "fds": 7,
        "threads": 3,
        "sessions": [9, 11],
    }


@contextmanager
def _server(path: Path, response: bytes) -> Generator[None]:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(str(path))
        listener.listen(1)

        def respond() -> None:
            connection = listener.accept()[0]
            with connection:
                assert connection.recv(64) == b"snapshot\n"
                try:
                    connection.sendall(response)
                except BrokenPipeError, ConnectionResetError:
                    return

        worker = threading.Thread(target=respond)
        worker.start()
        try:
            yield
        finally:
            worker.join(timeout=3)
            assert not worker.is_alive()


def test_query_reads_identity_and_resources_from_authenticated_peer(
    tmp_path: Path,
) -> None:
    endpoint = tmp_path / "status.sock"
    with _server(endpoint, json.dumps(_status()).encode() + b"\n"):
        snapshot = accepted(query(endpoint))
    assert snapshot.identity.pid == os.getpid()
    assert snapshot.identity.uid == os.geteuid()
    assert (snapshot.cpu_seconds, snapshot.fds, snapshot.threads) == (0.25, 7, 3)
    assert snapshot.sessions == (9, 11)


@pytest.mark.parametrize("field", ["pid", "uid"])
def test_query_rejects_identity_disagreeing_with_kernel_peer_credentials(
    tmp_path: Path, field: str
) -> None:
    status = _status()
    identity = status["identity"]
    assert isinstance(identity, dict)
    identity[field] = -1 if field == "pid" else os.geteuid() + 1
    endpoint = tmp_path / "status.sock"
    with _server(endpoint, json.dumps(status).encode() + b"\n"):
        result = query(endpoint)
    assert isinstance(result, Rejected) and result.code == errno.EPROTO


@pytest.mark.parametrize(
    "response",
    [b"", b"{}\n", b"not-json\n", b"x" * 65537, b'{"error":"counter read failed"}\n'],
    ids=[
        "empty-response",
        "missing-fields",
        "invalid-json",
        "oversized-response",
        "server-error",
    ],
)
def test_query_rejects_malformed_truncated_oversized_and_failed_responses(
    tmp_path: Path, response: bytes
) -> None:
    endpoint = tmp_path / "status.sock"
    with _server(endpoint, response):
        assert isinstance(query(endpoint), Rejected)


def test_query_missing_socket_reports_absence(tmp_path: Path) -> None:
    result = query(tmp_path / "missing.sock")
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_query_stale_socket_reports_connection_refused(tmp_path: Path) -> None:
    endpoint = tmp_path / "stale.sock"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(str(endpoint))
    result = query(endpoint)
    assert isinstance(result, Rejected) and result.code == errno.ECONNREFUSED


@pytest.mark.parametrize("version", [0, 2, True, 1.0, "1"])
def test_query_protocol_requires_exact_supported_integer(
    tmp_path: Path, version: object
) -> None:
    status = _status()
    status["protocol"] = version
    endpoint = tmp_path / "status.sock"
    with _server(endpoint, json.dumps(status).encode() + b"\n"):
        result = query(endpoint)
    assert isinstance(result, Rejected) and result.code == errno.EPROTO


@pytest.mark.parametrize("sessions", [[-1], [1, 1], ["3"]])
def test_query_invalid_session_inventory_is_rejected(
    tmp_path: Path, sessions: list[int] | list[str]
) -> None:
    status = _status()
    status["sessions"] = sessions
    endpoint = tmp_path / "status.sock"
    with _server(endpoint, json.dumps(status).encode() + b"\n"):
        assert isinstance(query(endpoint), Rejected)
