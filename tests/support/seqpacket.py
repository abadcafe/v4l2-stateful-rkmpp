"""module: seqpacket; responsibility: own bounded Unix message resources.

operations: Socket pair/listen/connect/accept/send/receive/close, Message close;
entities: Socket, Message; tests: seqpacket_tests, mock service acceptance.
"""

import array
import errno
import socket
from dataclasses import dataclass
from pathlib import Path
from types import TracebackType

from tests.support.outcome import Ok, Rejected, Result
from tests.support.syscalls import OwnedFd


@dataclass(frozen=True)
class Message:
    body: bytes
    fds: tuple[OwnedFd, ...] = ()

    def close(self) -> None:
        for owner in self.fds:
            owner.close()

    def __enter__(self) -> Message:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()


class Socket:
    def __init__(self, endpoint: socket.socket, path: Path | None = None) -> None:
        self._socket: socket.socket = endpoint
        self._path: Path | None = path
        self._closed: bool = False
        self._socket.settimeout(0.1)

    @classmethod
    def pair(cls) -> Result[tuple[Socket, Socket]]:
        try:
            left, right = socket.socketpair(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        except OSError as error:
            return Rejected(error.errno or 0, "create Unix pair")
        return Ok((cls(left), cls(right)))

    @classmethod
    def listen(cls, path: Path) -> Result[Socket]:
        try:
            endpoint = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        except OSError as error:
            return Rejected(error.errno or 0, "create Unix listener")
        try:
            endpoint.bind(str(path))
            endpoint.listen(32)
        except OSError as error:
            endpoint.close()
            return Rejected(error.errno or 0, "bind Unix listener")
        return Ok(cls(endpoint, path))

    @classmethod
    def connect(cls, path: Path) -> Result[Socket]:
        try:
            endpoint = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        except OSError as error:
            return Rejected(error.errno or 0, "create Unix connection")
        try:
            endpoint.settimeout(1)
            endpoint.connect(str(path))
        except OSError as error:
            endpoint.close()
            return Rejected(error.errno or errno.ETIMEDOUT, "connect Unix endpoint")
        return Ok(cls(endpoint))

    def accept(self) -> Result[Socket]:
        try:
            endpoint = self._socket.accept()[0]
        except OSError as error:
            return Rejected(error.errno or errno.ETIMEDOUT, "accept Unix endpoint")
        return Ok(Socket(endpoint))

    def send(self, body: bytes, fds: tuple[int, ...] = ()) -> Result[None]:
        if not 0 < len(body) <= 32768 or len(fds) > 32:
            return Rejected(errno.EINVAL, "invalid Unix message extent")
        ancillary = (
            [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", fds).tobytes())]
            if fds
            else []
        )
        try:
            written = self._socket.sendmsg([body], ancillary, socket.MSG_NOSIGNAL)
        except OSError as error:
            return Rejected(error.errno or errno.ETIMEDOUT, "send Unix message")
        return (
            Ok(None)
            if written == len(body)
            else Rejected(errno.EIO, "partial Unix message")
        )

    def receive(self) -> Result[Message]:
        try:
            received = self._socket.recvmsg(
                32768, socket.CMSG_SPACE(32 * 4), socket.MSG_CMSG_CLOEXEC
            )
        except OSError as error:
            return Rejected(error.errno or errno.ETIMEDOUT, "receive Unix message")
        body, ancillary, flags = received[:3]
        owners: list[OwnedFd] = []
        valid = True
        for level, kind, raw in ancillary:
            if level != socket.SOL_SOCKET or kind != socket.SCM_RIGHTS or len(raw) % 4:
                valid = False
                continue
            descriptors = array.array("i")
            descriptors.frombytes(raw)
            owners.extend(OwnedFd(value) for value in descriptors)
        if not valid or flags & (socket.MSG_TRUNC | socket.MSG_CTRUNC):
            for owner in owners:
                owner.close()
            return Rejected(errno.EMSGSIZE, "invalid Unix descriptor message")
        if not body:
            for owner in owners:
                owner.close()
            return Rejected(errno.EPIPE, "Unix peer closed")
        return Ok(Message(body, tuple(owners)))

    def close(self) -> None:
        if not self._closed:
            self._socket.close()
            if self._path is not None:
                self._path.unlink()
            self._closed = True

    def __enter__(self) -> Socket:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()
