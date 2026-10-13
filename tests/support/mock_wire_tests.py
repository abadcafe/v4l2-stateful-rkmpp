"""module: mock_wire_tests; responsibility: specify the private mock transport.

operations: pytest cases; entities: none; tests: request/reply/wake boundaries.
"""

import struct

import pytest

from tests.support.bridge_abi import ACCEPT, REGISTER_POOL
from tests.support.mock_wire import Patch, Reply, read_request, reply, request, wake
from tests.support.outcome import Rejected, accepted


def test_request_roundtrip_preserves_command_and_complete_payload() -> None:
    body = bytes(range(256)) + bytes(32)
    encoded = accepted(request(REGISTER_POOL, body))
    decoded = accepted(read_request(encoded))
    assert decoded.command == REGISTER_POOL and decoded.body == body
    empty = accepted(read_request(accepted(request(ACCEPT))))
    assert empty.command == ACCEPT and not empty.body


@pytest.mark.parametrize("raw", [b"", bytes(31), bytes(32), b"RBM2" + bytes(40)])
def test_read_request_invalid_header_is_rejected(raw: bytes) -> None:
    assert isinstance(read_request(raw), Rejected)


@pytest.mark.parametrize("offset", [4, 16, 20, 24, 28])
def test_read_request_inconsistent_extent_or_header_is_rejected(offset: int) -> None:
    raw = bytearray(accepted(request(ACCEPT)))
    raw[offset : offset + 4] = struct.pack("<I", 9)
    assert isinstance(read_request(bytes(raw)), Rejected)


def test_reply_preserves_fd_patches_and_allows_descriptor_aliases() -> None:
    value = Reply(body=bytes(8), fds=(7,), patches=(Patch(0, 0), Patch(4, 0)))
    encoded = accepted(reply(value))
    assert encoded == (
        struct.pack("<IIQIIII", 0x324D4252, 2, 0, 8, 1, 2, 0)
        + struct.pack("<IIII", 0, 0, 4, 0)
        + bytes(8)
    )


@pytest.mark.parametrize(
    "value",
    [
        Reply(code=-1),
        Reply(code=1, body=b"x"),
        Reply(body=bytes(4), fds=(-1,), patches=(Patch(0, 0),)),
        Reply(body=bytes(4), fds=(7,), patches=(Patch(0, 1),)),
        Reply(body=bytes(4), fds=(7,), patches=(Patch(1, 0),)),
        Reply(body=bytes(8), fds=(7,), patches=(Patch(0, 0), Patch(1, 0))),
        Reply(body=bytes(8), fds=(7,)),
    ],
)
def test_reply_invalid_ownership_or_error_payload_is_rejected(value: Reply) -> None:
    assert isinstance(reply(value), Rejected)


def test_wake_contains_no_payload_or_descriptor_ownership() -> None:
    assert wake() == struct.pack("<IIQIIII", 0x324D4252, 3, 0, 0, 0, 0, 0)


def test_request_invalid_command_or_payload_size_is_rejected() -> None:
    for command, body in ((-1, b""), (0, b""), (REGISTER_POOL, bytes(287))):
        assert isinstance(request(command, body), Rejected)
