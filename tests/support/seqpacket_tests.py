"""module: seqpacket_tests; responsibility: specify bounded message ownership.

operations: pytest cases; entities: none; tests: message/fd/endpoint lifetimes.
"""

import errno
import os
import socket
from pathlib import Path

import pytest

from tests.support.outcome import Rejected, accepted
from tests.support.seqpacket import Socket


def test_listen_failure_releases_bound_path_and_allows_retry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = tmp_path / "peer.sock"

    def fail_listen(endpoint: socket.socket, backlog: int = 0) -> None:
        assert endpoint.fileno() >= 0 and backlog == 32
        raise OSError(errno.ENOBUFS, "injected listener pressure")

    with monkeypatch.context() as fault:
        fault.setattr(socket.socket, "listen", fail_listen)
        result = Socket.listen(path)
    assert isinstance(result, Rejected) and result.code == errno.ENOBUFS
    assert not path.exists()
    with accepted(Socket.listen(path)):
        assert path.is_socket()
    assert not path.exists()


def test_pair_preserves_packet_boundaries_and_owned_cloexec_descriptors() -> None:
    left, right = accepted(Socket.pair())
    with left, right:
        fd = os.eventfd(0, os.EFD_CLOEXEC)
        try:
            assert accepted(left.send(b"one", (fd, fd))) is None
            assert accepted(left.send(b"two")) is None
            with accepted(right.receive()) as first:
                assert first.body == b"one" and len(first.fds) == 2
                numbers = tuple(owner.fd for owner in first.fds)
                assert len(set(numbers)) == 2 and fd not in numbers
                assert all(not os.get_inheritable(value) for value in numbers)
            for value in numbers:
                result = os.path.exists(f"/proc/self/fd/{value}")
                assert not result
            with accepted(right.receive()) as second:
                assert second.body == b"two" and second.fds == ()
        finally:
            os.close(fd)


def test_listen_connect_accept_cleanup_is_scoped_to_owned_endpoint(
    tmp_path: Path,
) -> None:
    path = tmp_path / "peer.sock"
    listener = accepted(Socket.listen(path))
    assert path.is_socket()
    with listener, accepted(Socket.connect(path)) as client:
        with accepted(listener.accept()) as server:
            _ = accepted(client.send(b"request"))
            with accepted(server.receive()) as message:
                assert message.body == b"request"
    assert not path.exists()
    accepted(listener.close())


def test_receive_timeout_and_closed_peer_are_distinct() -> None:
    left, right = accepted(Socket.pair())
    with left, right:
        timeout = right.receive()
        assert isinstance(timeout, Rejected) and timeout.code == errno.ETIMEDOUT
        accepted(left.close())
        closed = right.receive()
        assert isinstance(closed, Rejected) and closed.code == errno.EPIPE


def test_connect_missing_and_listen_existing_paths_are_rejected(tmp_path: Path) -> None:
    missing = Socket.connect(tmp_path / "missing")
    assert isinstance(missing, Rejected) and missing.code == errno.ENOENT
    occupied = tmp_path / "occupied"
    _ = occupied.write_bytes(b"preserve")
    result = Socket.listen(occupied)
    assert isinstance(result, Rejected) and result.code == errno.EADDRINUSE
    assert occupied.read_bytes() == b"preserve"


@pytest.mark.parametrize("replacement", [False, True])
def test_close_preserves_replaced_or_removed_listener_path(
    tmp_path: Path, replacement: bool
) -> None:
    path = tmp_path / "peer.sock"
    listener = accepted(Socket.listen(path))
    path.unlink()
    if replacement:
        _ = path.write_bytes(b"replacement")
    accepted(listener.close())
    accepted(listener.close())
    if replacement:
        assert path.read_bytes() == b"replacement"
    else:
        assert not path.exists()


@pytest.mark.parametrize(
    "body", [b"", bytes(32769)], ids=["empty-packet", "oversized-packet"]
)
def test_send_invalid_packet_extent_is_rejected(body: bytes) -> None:
    left, right = accepted(Socket.pair())
    with left, right:
        result = left.send(body)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_send_closed_descriptor_is_rejected_without_receiver_ownership() -> None:
    left, right = accepted(Socket.pair())
    with left, right:
        fd = os.eventfd(0, os.EFD_CLOEXEC)
        os.close(fd)
        result = left.send(b"packet", (fd,))
        assert isinstance(result, Rejected) and result.code == errno.EBADF


@pytest.mark.parametrize("fd", [-(1 << 128), -1, 1 << 31, 1 << 128])
def test_send_unrepresentable_descriptor_is_rejected_without_sending(fd: int) -> None:
    left, right = accepted(Socket.pair())
    with left, right:
        result = left.send(b"invalid", (fd,))
        assert isinstance(result, Rejected) and result.code == errno.EBADF
        accepted(left.send(b"after"))
        with accepted(right.receive()) as message:
            assert message.body == b"after" and message.fds == ()
