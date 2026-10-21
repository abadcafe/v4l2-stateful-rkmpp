"""module: mock_process; responsibility: own a daemon's simulated kernel endpoint.

operations: Process open/session/reject_accept/log/close, Peer wait/wake/calls/close;
entities: Process, Peer, Handler; tests: mock_process_tests, service acceptance.
Only the process environment contains the preload selection and private endpoints.
Explicit inherited descriptors remain owned by the caller and are copied into the child.
"""

import errno
import math
import os
import queue
import subprocess
import threading
import time
from collections.abc import Callable
from concurrent.futures import Future
from contextlib import ExitStack
from pathlib import Path
from types import TracebackType

from rkmpp_bridge_uapi import ACCEPT

from tests.support.mock_wire import Reply, Request, read_request, reply, wake
from tests.support.outcome import Ok, Rejected, Result
from tests.support.seqpacket import Socket
from tests.support.syscalls import OwnedFd

type Handler = Callable[[Request], Reply]
type _Offer = tuple[Handler, Future[Peer]] | int


class Peer:
    def __init__(self, endpoint: Socket, handler: Handler) -> None:
        self._endpoint: Socket = endpoint
        self._handler: Handler = handler
        self._stop: threading.Event = threading.Event()
        self._done: threading.Event = threading.Event()
        self._calls: list[Request] = []
        self._lock: threading.Lock = threading.Lock()
        self._result: Result[None] = Ok(None)
        self._thread: threading.Thread = threading.Thread(target=self._run)
        self._thread.start()

    def _run(self) -> None:
        try:
            while not self._stop.is_set():
                received = self._endpoint.receive()
                if isinstance(received, Rejected):
                    if received.code == errno.ETIMEDOUT:
                        continue
                    if received.code not in (errno.EPIPE, errno.EBADF):
                        self._result = received
                    return
                with received.value as message:
                    request = read_request(message.body)
                    if isinstance(request, Rejected):
                        self._result = request
                        return
                    with self._lock:
                        self._calls.append(request.value)
                    response = self._handler(request.value)
                    encoded = reply(response)
                    if isinstance(encoded, Rejected):
                        self._result = encoded
                        return
                    sent = self._endpoint.send(encoded.value, response.fds)
                    if isinstance(sent, Rejected):
                        self._result = sent
                        return
        finally:
            assert isinstance(self._endpoint.close(), Ok)
            self._done.set()

    def wait(self, timeout: float) -> Result[None]:
        if not math.isfinite(timeout) or timeout <= 0:
            return Rejected(errno.EINVAL, "invalid peer deadline")
        if not self._done.wait(timeout):
            return Rejected(errno.ETIMEDOUT, "mock peer did not close")
        return self._result

    def wake(self) -> Result[None]:
        return self._endpoint.send(wake())

    def calls(self) -> tuple[Request, ...]:
        with self._lock:
            return tuple(self._calls)

    def close(self) -> None:
        self._stop.set()
        assert isinstance(self._endpoint.close(), Ok)
        self._thread.join(timeout=5)
        assert not self._thread.is_alive()


class Process:
    def __init__(
        self,
        process: subprocess.Popen[bytes],
        listener: Socket,
        fifo: OwnedFd,
        log: Path,
        lease: ExitStack,
    ) -> None:
        self.process: subprocess.Popen[bytes] = process
        self.diagnostics: Path = log.parent / "d"
        self.thread_failure: Path = log.parent / "thread-failure"
        self._listener: Socket = listener
        self._fifo: OwnedFd = fifo
        self._log: Path = log
        self._lease: ExitStack = lease
        self._offers: queue.Queue[_Offer] = queue.Queue()
        self._peers: list[Peer] = []
        self._stop: threading.Event = threading.Event()
        self._thread: threading.Thread = threading.Thread(target=self._accept)
        self._thread.start()

    @classmethod
    def open(
        cls,
        executable: Path,
        library: Path,
        directory: Path,
        *,
        inherited: tuple[OwnedFd, ...] = (),
    ) -> Result[Process]:
        if not executable.is_file() or not library.is_file():
            return Rejected(errno.ENOENT, "build daemon and mock preload library first")
        lease = ExitStack()
        try:
            directory.mkdir(mode=0o700)
            control = directory / "ctl"
            os.mkfifo(control, mode=0o600)
            fifo = lease.enter_context(
                OwnedFd(os.open(control, os.O_RDWR | os.O_NONBLOCK | os.O_CLOEXEC))
            )
            endpoint = directory / "s"
            listen = Socket.listen(endpoint)
            if isinstance(listen, Rejected):
                lease.close()
                return listen
            listener = lease.enter_context(listen.value)
            log = directory / "service.log"
            stream = lease.enter_context(log.open("wb"))
            environment = dict(os.environ)
            environment.update(
                LD_PRELOAD=str(library.resolve()),
                RKMPP_TEST_KERNEL_SOCKET=str(endpoint.resolve()),
                RKMPP_TEST_KERNEL_CONTROL=str(control.resolve()),
                RKMPP_TEST_THREAD_FAILURE=str((directory / "thread-failure").resolve()),
            )
            process = subprocess.Popen(
                [
                    str(executable),
                    "serve",
                    "/dev/v4l2-rkmpp-control",
                    str(directory / "d"),
                ],
                env=environment,
                pass_fds=tuple(owner.fd for owner in inherited),
                stdout=stream,
                stderr=stream,
            )
        except OSError as error:
            lease.close()
            return Rejected(error.errno or 0, "start mocked service process")
        result = cls(process, listener, fifo, log, lease)
        deadline = time.monotonic() + 10
        while process.poll() is None and time.monotonic() < deadline:
            if "service listening" in result.log():
                return Ok(result)
            time.sleep(0.01)
        details = result.log()
        result.close()
        return Rejected(errno.EIO, "mocked service startup: " + details)

    def _accept(self) -> None:
        while not self._stop.is_set():
            accepted = self._listener.accept()
            if isinstance(accepted, Rejected):
                if accepted.code == errno.ETIMEDOUT:
                    continue
                return
            endpoint = accepted.value
            received = endpoint.receive()
            if isinstance(received, Rejected):
                assert isinstance(endpoint.close(), Ok)
                continue
            with received.value as message:
                request = read_request(message.body)
                assert isinstance(request, Ok) and request.value.command == ACCEPT
            try:
                offer = self._offers.get_nowait()
            except queue.Empty:
                encoded = reply(Reply(code=errno.EAGAIN))
                assert isinstance(encoded, Ok)
                _ = endpoint.send(encoded.value)
                assert isinstance(endpoint.close(), Ok)
                continue
            assert os.read(self._fifo.fd, 1) == b"1"
            if isinstance(offer, int):
                encoded = reply(Reply(code=offer))
                assert isinstance(encoded, Ok)
                _ = endpoint.send(encoded.value)
                assert isinstance(endpoint.close(), Ok)
                continue
            handler, future = offer
            encoded = reply(Reply())
            assert isinstance(encoded, Ok)
            sent = endpoint.send(encoded.value)
            if isinstance(sent, Rejected):
                assert isinstance(endpoint.close(), Ok)
                continue
            peer = Peer(endpoint, handler)
            self._peers.append(peer)
            future.set_result(peer)

    def session(self, handler: Handler) -> Result[Peer]:
        future: Future[Peer] = Future()
        self._offers.put((handler, future))
        try:
            assert os.write(self._fifo.fd, b"1") == 1
            return Ok(future.result(timeout=5))
        except TimeoutError:
            return Rejected(errno.ETIMEDOUT, "daemon did not accept mock session")
        except OSError as error:
            return Rejected(error.errno or 0, "wake mock broker")

    def reject_accept(self, code: int) -> Result[None]:
        if not 0 < code <= 0x7FFFFFFF:
            return Rejected(errno.EINVAL, "invalid accept errno")
        self._offers.put(code)
        try:
            assert os.write(self._fifo.fd, b"1") == 1
        except OSError as error:
            return Rejected(error.errno or 0, "wake rejecting broker")
        return Ok(None)

    def log(self) -> str:
        return self._log.read_text(errors="replace")

    def close(self) -> None:
        self._stop.set()
        if self.process.poll() is None:
            self.process.terminate()
        for peer in self._peers:
            peer.close()
        assert isinstance(self._listener.close(), Ok)
        self._thread.join(timeout=5)
        assert not self._thread.is_alive()
        if self.process.poll() is None:
            try:
                _ = self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                _ = self.process.wait(timeout=5)
        self._lease.close()

    def __enter__(self) -> Process:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()
