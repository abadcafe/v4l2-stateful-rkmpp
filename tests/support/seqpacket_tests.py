"""module: seqpacket_tests; responsibility: specify bounded message ownership.

operations: pytest cases; entities: none; tests: message/fd/endpoint lifetimes.
"""

import errno
import os
from pathlib import Path

import pytest

from tests.support.outcome import Rejected, accepted
from tests.support.seqpacket import Socket


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
    listener.close()


def test_receive_timeout_and_closed_peer_are_distinct() -> None:
    left, right = accepted(Socket.pair())
    with left, right:
        timeout = right.receive()
        assert isinstance(timeout, Rejected) and timeout.code == errno.ETIMEDOUT
        left.close()
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
