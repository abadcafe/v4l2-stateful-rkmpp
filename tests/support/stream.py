"""module: stream; responsibility: validate compressed acceptance inputs.

operations: Codec, Packet, parse, load; entities: Codec, Packet;
tests: stream_tests, v4l2/test_decode.
"""

import errno
import struct
from enum import StrEnum
from pathlib import Path

from pydantic import BaseModel

from tests.support.outcome import Ok, Rejected, Result


class Codec(StrEnum):
    H264 = "H264"
    HEVC = "HEVC"
    VP8 = "VP80"
    VP9 = "VP90"
    AV1 = "AV01"

    @property
    def fourcc(self) -> int:
        return int.from_bytes(self.value.encode(), "little")


class Packet(BaseModel):
    data: bytes
    timestamp: int


def parse(codec: Codec, data: bytes) -> Result[tuple[Packet, ...]]:
    if not data or len(data) > 16 * 1024 * 1024:
        return Rejected(errno.EINVAL, "empty or oversized compressed input")
    if codec in (Codec.H264, Codec.HEVC):
        return Ok((Packet(data=data, timestamp=0),))
    if len(data) < 32 or data[:4] != b"DKIF":
        return Rejected(errno.EINVAL, "truncated IVF header")
    version, length, fourcc = struct.unpack_from("<HH4s", data, 4)
    rate, scale, count = struct.unpack_from("<III", data, 16)
    if (
        version != 0
        or length != 32
        or fourcc != codec.value.encode()
        or not rate
        or not scale
    ):
        return Rejected(errno.EINVAL, "invalid IVF header")
    packets: list[Packet] = []
    offset = 32
    while offset < len(data):
        if offset + 12 > len(data):
            return Rejected(errno.EINVAL, "truncated IVF frame header")
        size, timestamp = struct.unpack_from("<IQ", data, offset)
        offset += 12
        if size == 0 or offset + size > len(data):
            return Rejected(errno.EINVAL, "truncated IVF frame payload")
        packets.append(
            Packet(
                data=data[offset : offset + size],
                timestamp=timestamp * scale * 1000000 // rate,
            )
        )
        offset += size
    if not packets or (count and count != len(packets)):
        return Rejected(errno.EINVAL, "invalid IVF frame count")
    return Ok(tuple(packets))


def load(codec: Codec, path: Path) -> Result[tuple[Packet, ...]]:
    try:
        data = path.read_bytes()
    except OSError as error:
        return Rejected(error.errno or 0, f"read compressed input {path}")
    return parse(codec, data)
