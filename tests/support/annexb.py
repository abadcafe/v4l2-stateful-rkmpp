"""module: annexb; responsibility: client-side Annex B access-unit framing.

operations: packets; entities: PacketExtent, Probe; tests: annexb_tests, stream_tests.
FFprobe is the independent test client's demuxer, never part of the service.
"""

import errno
import subprocess

from pydantic import BaseModel, ValidationError

from tests.support.outcome import Ok, Rejected, Result


class PacketExtent(BaseModel):
    pos: int
    size: int


class Probe(BaseModel):
    packets: tuple[PacketExtent, ...] = ()


def _termination_prefix(packet: bytes, codec: str) -> int:
    offset = 0
    while offset < len(packet):
        start = packet.find(b"\x00\x00\x01", offset)
        if start < 0 or start + 3 >= len(packet) or any(packet[offset:start]):
            break
        kind = (
            packet[start + 3] & 31 if codec == "h264" else (packet[start + 3] >> 1) & 63
        )
        if kind not in ((10, 11) if codec == "h264" else (36, 37)):
            break
        following = packet.find(b"\x00\x00\x01", start + 3)
        offset = len(packet) if following < 0 else following
    return offset


def packets(data: bytes, codec: str) -> Result[tuple[bytes, ...]]:
    try:
        probe = subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-f",
                codec,
                "-i",
                "pipe:0",
                "-show_packets",
                "-show_entries",
                "packet=pos,size",
                "-of",
                "json",
            ],
            input=data,
            capture_output=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return Rejected(errno.EIO, f"frame Annex B input: {error}")
    try:
        extents = Probe.model_validate_json(probe.stdout).packets
    except ValidationError as error:
        return Rejected(errno.EINVAL, f"invalid demuxer packet report: {error}")
    # Configuration-only and deliberately damaged test packets have no
    # demuxed picture. Submit their bytes as one explicit metadata/input unit
    # so the service, rather than the test demuxer, decides their admissibility.
    if not extents:
        return Ok((data,))
    output: list[bytes] = []
    previous = 0
    for extent in extents:
        if (
            extent.pos != previous
            or extent.size <= 0
            or extent.pos + extent.size > len(data)
        ):
            return Rejected(errno.EINVAL, "noncontiguous Annex B packet extents")
        end = extent.pos + extent.size
        packet = data[extent.pos : end]
        # EOS/EOB terminate the preceding AU. FFprobe may place them at
        # the start of its next packet or in a separate final packet.
        # Preserve the complete AU, including consecutive terminators;
        # never split a picture's valid termination into metadata inputs.
        prefix = _termination_prefix(packet, codec)
        if prefix:
            if output:
                output[-1] += packet[:prefix]
            else:
                output.append(packet[:prefix])
        if prefix < len(packet):
            output.append(packet[prefix:])
        previous = end
    if previous != len(data):
        return Rejected(errno.EINVAL, "unconsumed Annex B input")
    return Ok(tuple(output))
