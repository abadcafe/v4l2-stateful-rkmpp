"""module: diagnostics; responsibility: query the service's authenticated status.

operations: Control, Identity, Snapshot, query; entities: Control, Identity,
Snapshot, _Failure; tests: diagnostics_tests, service/test_diagnostics.
No control descriptors or mutable commands cross this socket.
"""

import errno
import socket
import struct
import time
from pathlib import Path
from typing import ClassVar, Literal

from pydantic import BaseModel, ConfigDict, Field, TypeAdapter, ValidationError

from tests.support.outcome import Ok, Rejected, Result


class Control(BaseModel, frozen=True):
    model_config: ClassVar[ConfigDict] = ConfigDict(extra="forbid")
    device: int = Field(ge=0)
    inode: int = Field(ge=0)
    raw_device: int = Field(ge=0)


class Identity(BaseModel, frozen=True):
    model_config: ClassVar[ConfigDict] = ConfigDict(extra="forbid")
    pid: int = Field(gt=0)
    uid: int = Field(ge=0)
    start_ticks: int = Field(gt=0)
    build_id: str = Field(pattern=r"^[0-9a-f]{16,128}$")
    control: Control


class Measurement(BaseModel, frozen=True):
    model_config: ClassVar[ConfigDict] = ConfigDict(extra="forbid")
    protocol: int = Field(ge=1, le=1, strict=True)
    identity: Identity
    state: Literal["ready", "stopping"]
    cpu_seconds: float = Field(ge=0, allow_inf_nan=False)


class Snapshot(Measurement, frozen=True):
    fds: int = Field(gt=0)
    threads: int = Field(gt=0)
    sessions: tuple[int, ...]


class _Failure(BaseModel):
    model_config: ClassVar[ConfigDict] = ConfigDict(extra="forbid")
    error: str


def query(endpoint: Path) -> Result[Snapshot]:
    return _query(endpoint, b"snapshot\n", TypeAdapter(Snapshot | _Failure))


def measure(endpoint: Path) -> Result[Measurement]:
    return _query(endpoint, b"measure\n", TypeAdapter(Measurement | _Failure))


def _query[T: Measurement](
    endpoint: Path, request: bytes, schema: TypeAdapter[T | _Failure]
) -> Result[T]:
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(1)
            client.connect(str(endpoint))
            credentials = client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
            pid, uid, _ = TypeAdapter(tuple[int, int, int]).validate_python(
                struct.unpack("3i", credentials), strict=True
            )
            client.sendall(request)
            response = bytearray()
            deadline = time.monotonic() + 1
            while len(response) <= 65536:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return Rejected(
                        errno.ETIMEDOUT, f"Diagnostics endpoint {endpoint} timed out"
                    )
                client.settimeout(remaining)
                packet = client.recv(min(4096, 65537 - len(response)))
                if not packet:
                    break
                response.extend(packet)
                # The snapshot excludes this query's transient socket. Wait for
                # EOF so callers can compare it with the live descriptor table.
    except TimeoutError:
        return Rejected(errno.ETIMEDOUT, f"Diagnostics endpoint {endpoint} timed out")
    except OSError as error:
        return Rejected(
            error.errno or 0, f"Query service diagnostics endpoint {endpoint}: {error}"
        )
    if len(response) > 65536 or not response.endswith(b"\n"):
        return Rejected(
            errno.EPROTO,
            "Diagnostics response is missing, truncated or exceeds the size limit",
        )
    try:
        status = schema.validate_json(bytes(response), strict=True)
    except ValidationError as error:
        return Rejected(errno.EPROTO, f"Invalid service diagnostics response: {error}")
    if isinstance(status, _Failure):
        return Rejected(errno.EIO, "Service diagnostics failed: " + status.error)
    if status.identity.pid != pid or status.identity.uid != uid:
        return Rejected(
            errno.EPROTO,
            "Diagnostics identity does not match kernel SO_PEERCRED credentials",
        )
    result: Result[T] = Ok(status)
    if isinstance(status, Snapshot) and (
        any(fd < 0 for fd in status.sessions)
        or len(set(status.sessions)) != len(status.sessions)
    ):
        return Rejected(errno.EPROTO, "Invalid service session descriptors")
    return result
