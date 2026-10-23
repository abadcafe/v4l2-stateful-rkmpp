"""module: cdp.transport; responsibility: own deadline-bounded WebSocket I/O.

operations: Transport open/send/receive/close; entities: Transport;
tests: transport_tests, cdp_tests. A single owned event-loop thread advances
the protocol between calls; asynchronous I/O bounds both writes and reads.
"""

import asyncio
import errno
import math
import threading
from collections.abc import Coroutine
from concurrent.futures import Future
from typing import override

from websockets.asyncio.client import ClientConnection, connect
from websockets.exceptions import ConnectionClosed, InvalidHandshake, InvalidURI

from tests.support.outcome import Ok, Rejected, Result

_CLOSE_TIMEOUT = 0.1


class _Connection(ClientConnection):
    _wire: asyncio.Transport | None = None

    @override
    def connection_made(self, transport: asyncio.BaseTransport) -> None:
        # asyncio owns initialization through its public Protocol callback.
        assert isinstance(transport, asyncio.Transport)
        self._wire = transport
        super().connection_made(transport)

    def abort(self) -> None:
        assert self._wire is not None
        self._wire.abort()


class Transport:
    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        connection: ClientConnection,
        stop: asyncio.Event,
        finished: Future[None],
    ) -> None:
        self._loop: asyncio.AbstractEventLoop = loop
        self._connection: ClientConnection = connection
        self._stop: asyncio.Event = stop
        self._finished: Future[None] = finished
        self._thread: threading.Thread = threading.current_thread()
        self._failure: Rejected | None = None

    @classmethod
    def open(cls, url: str) -> Result[Transport]:
        ready: Future[Result[Transport]] = Future()
        finished: Future[None] = Future()
        thread = threading.Thread(
            target=_run, args=(url, ready, finished), name="cdp-io"
        )
        thread.start()
        result: Result[Transport] | None = None
        try:
            while not ready.done():
                thread.join(timeout=0.01)
                assert thread.is_alive() or ready.done(), (
                    "CDP owner exited during startup"
                )
            result = ready.result()
        finally:
            if not isinstance(result, Ok):
                if result is None:
                    ready.add_done_callback(cls._abandon)
                thread.join()
                if finished.done():
                    finished.result()
        assert result is not None
        return result

    def _call[T](self, operation: Coroutine[object, object, Result[T]]) -> Result[T]:
        if self._failure is not None:
            operation.close()
            return self._failure
        return asyncio.run_coroutine_threadsafe(operation, self._loop).result()

    def send(self, data: str, timeout: float) -> Result[None]:
        if not math.isfinite(timeout) or timeout <= 0:
            return Rejected(errno.EINVAL, "invalid WebSocket send deadline")
        result = self._call(self._send(data, timeout))
        return self._retire(result) if isinstance(result, Rejected) else result

    async def _send(self, data: str, timeout: float) -> Result[None]:
        try:
            async with asyncio.timeout(timeout):
                await self._connection.send(data)
        except TimeoutError:
            return Rejected(errno.ETIMEDOUT, "WebSocket send deadline")
        except (ConnectionClosed, OSError) as error:
            return Rejected(errno.ECONNRESET, str(error))
        return Ok(None)

    def receive(self, timeout: float) -> Result[str | bytes]:
        if not math.isfinite(timeout) or timeout <= 0:
            return Rejected(errno.EINVAL, "invalid WebSocket receive deadline")
        result = self._call(self._receive(timeout))
        if isinstance(result, Rejected) and result.code != errno.ETIMEDOUT:
            return self._retire(result)
        return result

    async def _receive(self, timeout: float) -> Result[str | bytes]:
        try:
            async with asyncio.timeout(timeout):
                return Ok(await self._connection.recv())
        except TimeoutError:
            return Rejected(errno.ETIMEDOUT, "WebSocket receive deadline")
        except (ConnectionClosed, OSError) as error:
            return Rejected(errno.ECONNRESET, str(error))

    def close(self) -> None:
        if self._failure is None:
            self._failure = Rejected(errno.ECONNRESET, "WebSocket transport closed")
            _ = self._loop.call_soon_threadsafe(self._stop.set)
            self._thread.join()
            self._finished.result()

    def _retire(self, error: Rejected) -> Rejected:
        self.close()
        self._failure = error
        return error

    @staticmethod
    def _abandon(ready: Future[Result[Transport]]) -> None:
        if ready.exception() is None:
            opened = ready.result()
            if isinstance(opened, Ok):
                owner = opened.value
                _ = owner._loop.call_soon_threadsafe(owner._stop.set)


async def _lifetime(
    url: str, ready: Future[Result[Transport]], finished: Future[None]
) -> None:
    try:
        connection = await connect(
            url,
            proxy=None,
            open_timeout=5,
            close_timeout=_CLOSE_TIMEOUT,
            max_size=32 * 1024 * 1024,
            create_connection=_Connection,
        )
    except InvalidURI as error:
        ready.set_result(Rejected(errno.EINVAL, str(error)))
        return
    except (OSError, InvalidHandshake, TimeoutError) as error:
        ready.set_result(Rejected(errno.ECONNREFUSED, str(error)))
        return
    stop = asyncio.Event()
    assert isinstance(connection, _Connection)
    transport = Transport(asyncio.get_running_loop(), connection, stop, finished)
    ready.set_result(Ok(transport))
    try:
        _ = await stop.wait()
    finally:
        try:
            # websockets' close_timeout doesn't cover waiting for a full
            # write buffer to drain. Bound the entire close operation,
            # then release our owned asyncio transport without peer input.
            async with asyncio.timeout(_CLOSE_TIMEOUT):
                await connection.close()
        except TimeoutError:
            connection.abort()
            await connection.wait_closed()


def _run(url: str, ready: Future[Result[Transport]], finished: Future[None]) -> None:
    runner = asyncio.Runner()
    try:
        loop = runner.get_loop()
    except OSError as error:
        ready.set_result(
            Rejected(error.errno or errno.EIO, f"create CDP I/O loop: {error}")
        )
        finished.set_result(None)
        return
    with runner:
        task = loop.create_task(_lifetime(url, ready, finished))

        def completed(result: asyncio.Task[None]) -> None:
            # Publish unexpected failures through the public task result.
            # Programming errors aren't caught or converted into I/O errors.
            error = result.exception()
            if error is not None:
                if not ready.done():
                    ready.set_exception(error)
                finished.set_exception(error)
            else:
                finished.set_result(None)

        task.add_done_callback(completed)
        runner.run(_wait(task))


async def _wait(task: asyncio.Task[None]) -> None:
    await task
