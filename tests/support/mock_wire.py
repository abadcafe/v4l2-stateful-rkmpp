"""module: mock_wire; responsibility: encode opaque mock ioctl transfers.

operations: request/read_request/reply/wake, Request/Reply/Patch;
entities: Request, Reply, Patch; tests: mock_wire_tests, service process cases.
This wire belongs to the test interposer, independently of the bridge UAPI.
"""

import errno
import struct
from dataclasses import dataclass

from tests.support.outcome import Ok, Rejected, Result

_HEADER = struct.Struct("<IIQIIII")


@dataclass(frozen=True)
class Request:
    command: int
    body: bytes


@dataclass(frozen=True)
class Patch:
    offset: int
    index: int


@dataclass(frozen=True)
class Reply:
    code: int = 0
    body: bytes = b""
    fds: tuple[int, ...] = ()
    patches: tuple[Patch, ...] = ()


def request(command: int, body: bytes = b"") -> Result[bytes]:
    if (
        not 0 <= command <= 0xFFFFFFFFFFFFFFFF
        or command & 0xFF00 != 0x5200
        or len(body) != (command >> 16) & 0x3FFF
    ):
        return Rejected(errno.EINVAL, "invalid mock ioctl request")
    return Ok(_HEADER.pack(0x324D4252, 1, command, len(body), 0, 0, 0) + body)


def read_request(raw: bytes) -> Result[Request]:
    if len(raw) < _HEADER.size:
        return Rejected(errno.EPROTO, "truncated mock ioctl header")
    values: tuple[int, ...] = _HEADER.unpack_from(raw)
    magic, kind, command, length, fds, patches, reserved = values
    if (
        magic != 0x324D4252
        or kind != 1
        or fds != 0
        or patches != 0
        or reserved != 0
        or len(raw) != _HEADER.size + length
    ):
        return Rejected(errno.EPROTO, "invalid mock ioctl header")
    body = raw[_HEADER.size :]
    validated = request(command, body)
    if isinstance(validated, Rejected):
        return Rejected(errno.EPROTO, validated.operation)
    return Ok(Request(command, body))


def reply(value: Reply) -> Result[bytes]:
    if (
        not 0 <= value.code <= 0x7FFFFFFF
        or len(value.fds) > 32
        or len(value.patches) > 64
        or len(value.body) > 16384
        or any(fd < 0 for fd in value.fds)
        or (value.code != 0 and (value.body or value.fds or value.patches))
    ):
        return Rejected(errno.EINVAL, "invalid mock ioctl response")
    offsets: list[int] = []
    used: set[int] = set()
    for patch in value.patches:
        if (
            not 0 <= patch.index < len(value.fds)
            or not 0 <= patch.offset <= len(value.body) - 4
            or any(patch.offset < old + 4 and old < patch.offset + 4 for old in offsets)
        ):
            return Rejected(errno.EINVAL, "invalid descriptor patch")
        offsets.append(patch.offset)
        used.add(patch.index)
    if len(used) != len(value.fds):
        return Rejected(errno.EINVAL, "unreferenced descriptor ownership")
    return Ok(
        _HEADER.pack(
            0x324D4252,
            2,
            value.code,
            len(value.body),
            len(value.fds),
            len(value.patches),
            0,
        )
        + b"".join(
            struct.pack("<II", patch.offset, patch.index) for patch in value.patches
        )
        + value.body
    )


def wake() -> bytes:
    return _HEADER.pack(0x324D4252, 3, 0, 0, 0, 0, 0)
