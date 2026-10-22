"""module: video; responsibility: own a V4L2 session's control interface.

operations: Video open/capability/subscribe/events/stop/start/poll/close;
entities: Video; tests: video_tests, kernel/test_lifecycle, service/test_recovery.
"""

import ctypes
import errno
import select
from pathlib import Path
from types import TracebackType

from tests.support.event_abi import DecoderCommand, Event, Subscription
from tests.support.format_abi import Capability
from tests.support.outcome import Ok, Rejected, Result
from tests.support.syscalls import OwnedFd, command, ioctl, open_fd


class Video:
    def __init__(self, owner: OwnedFd) -> None:
        self._owner: OwnedFd = owner

    @classmethod
    def open(cls, path: Path) -> Result[Video]:
        result = open_fd(path)
        return Ok(cls(result.value)) if isinstance(result, Ok) else result

    @property
    def fd(self) -> int:
        return self._owner.fd

    def capability(self) -> Result[Capability]:
        payload = Capability()
        result = ioctl(self.fd, command(2, "V", 0, ctypes.sizeof(payload)), payload)
        return Ok(payload) if isinstance(result, Ok) else result

    def subscribe(self, kind: int) -> Result[None]:
        if not 0 <= kind < 1 << 32:
            return Rejected(errno.EINVAL, "event type outside unsigned 32-bit ABI")
        payload = Subscription(kind=kind)
        result = ioctl(self.fd, command(1, "V", 90, ctypes.sizeof(payload)), payload)
        return Ok(None) if isinstance(result, Ok) else result

    def events(self) -> Result[list[tuple[int, int]]]:
        result: list[tuple[int, int]] = []
        while True:
            payload = Event()
            status = ioctl(
                self.fd, command(2, "V", 89, ctypes.sizeof(payload)), payload
            )
            if isinstance(status, Rejected):
                return Ok(result) if status.code == errno.ENOENT else status
            result.append((payload.kind, payload.payload.changes))

    def stop(self) -> Result[None]:
        payload = DecoderCommand(command=1)
        result = ioctl(self.fd, command(3, "V", 96, ctypes.sizeof(payload)), payload)
        return Ok(None) if isinstance(result, Ok) else result

    def start(self) -> Result[None]:
        payload = DecoderCommand(command=0)
        result = ioctl(self.fd, command(3, "V", 96, ctypes.sizeof(payload)), payload)
        return Ok(None) if isinstance(result, Ok) else result

    def poll(self, timeout_ms: int) -> int:
        poller = select.poll()
        poller.register(
            self.fd, select.POLLIN | select.POLLPRI | select.POLLOUT | select.POLLERR
        )
        return sum(mask for _, mask in poller.poll(timeout_ms))

    def close(self) -> None:
        self._owner.close()

    def __enter__(self) -> Video:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()
