"""module: cdp; responsibility: exchange typed Chromium debugging messages.

operations: Cdp open/call/evaluate/events/close; entities: Cdp, Notification,
Payload; tests: cdp_tests (success, errors, timeout, event order).
"""

import errno
import json
import math
import time
from contextlib import ExitStack
from types import TracebackType

from pydantic import BaseModel, Field, JsonValue, TypeAdapter, ValidationError
from websockets.exceptions import ConnectionClosed, InvalidHandshake, InvalidURI
from websockets.sync.client import ClientConnection, connect

from tests.support.outcome import Ok, Rejected, Result

type Payload = dict[str, JsonValue]

_MAX_EVENTS = 1024
_MAX_EVENT_BYTES = 8 * 1024 * 1024
_MAX_EXPIRED = 64


class _WireError(BaseModel):
    code: int
    message: str


class _Envelope(BaseModel):
    id: int | None = None
    method: str | None = None
    params: Payload = Field(default_factory=dict)
    result: Payload | None = None
    error: _WireError | None = None


class Notification(BaseModel):
    method: str
    params: Payload


class _RemoteObject(BaseModel):
    value: JsonValue = None


class _Evaluation(BaseModel):
    result: _RemoteObject = Field(default_factory=_RemoteObject)
    exceptionDetails: Payload | None = None


class Cdp:
    def __init__(self, connection: ClientConnection, lease: ExitStack) -> None:
        self._connection: ClientConnection = connection
        self._lease: ExitStack = lease
        self._sequence: int = 0
        self._expired: set[int] = set()
        self._events: list[Notification] = []
        self._event_bytes: int = 0
        self._failure: Rejected | None = None

    @classmethod
    def open(cls, url: str) -> Result[Cdp]:
        lease = ExitStack()
        try:
            connection = lease.enter_context(
                connect(
                    url,
                    proxy=None,
                    open_timeout=5,
                    close_timeout=1,
                    max_size=32 * 1024 * 1024,
                )
            )
        except InvalidURI as error:
            return Rejected(errno.EINVAL, str(error))
        except (OSError, InvalidHandshake, TimeoutError) as error:
            return Rejected(errno.ECONNREFUSED, str(error))
        assert isinstance(connection, ClientConnection)
        return Ok(cls(connection, lease))

    def call(
        self, method: str, params: Payload | None = None, *, timeout: float = 10
    ) -> Result[Payload]:
        if not math.isfinite(timeout) or timeout <= 0:
            return Rejected(errno.EINVAL, "invalid CDP deadline")
        if self._failure is not None:
            return self._failure
        if len(self._expired) >= _MAX_EXPIRED:
            return self._overflow("too many unanswered CDP commands")
        self._sequence += 1
        deadline = time.monotonic() + timeout
        try:
            self._connection.send(
                json.dumps(
                    {"id": self._sequence, "method": method, "params": params or {}}
                )
            )
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    self._expired.add(self._sequence)
                    return Rejected(errno.ETIMEDOUT, "CDP " + method)
                raw = self._connection.recv(timeout=remaining)
                try:
                    envelope = _Envelope.model_validate_json(raw, strict=True)
                except ValidationError as error:
                    return Rejected(errno.EPROTO, str(error))
                if envelope.id is None:
                    if envelope.method is None:
                        return Rejected(
                            errno.EPROTO, "CDP response has neither id nor method"
                        )
                    size = len(raw.encode()) if isinstance(raw, str) else len(raw)
                    if (
                        len(self._events) >= _MAX_EVENTS
                        or self._event_bytes + size > _MAX_EVENT_BYTES
                    ):
                        return self._overflow(
                            "unconsumed CDP notifications exceed capacity"
                        )
                    self._events.append(
                        Notification(method=envelope.method, params=envelope.params)
                    )
                    self._event_bytes += size
                    continue
                if envelope.id in self._expired:
                    # Timed-out commands may finish after the next call starts.
                    # They cannot satisfy the current command's response.
                    self._expired.remove(envelope.id)
                    continue
                if envelope.id != self._sequence:
                    return Rejected(errno.EPROTO, "unexpected CDP response id")
                if envelope.error is not None:
                    return Rejected(envelope.error.code, envelope.error.message)
                if envelope.result is None:
                    return Rejected(errno.EPROTO, "CDP reply has no result or error")
                return Ok(envelope.result)
        except TimeoutError:
            self._expired.add(self._sequence)
            return Rejected(errno.ETIMEDOUT, "CDP " + method)
        except (ConnectionClosed, OSError) as error:
            return Rejected(errno.ECONNRESET, str(error))

    def evaluate[T](self, expression: str, model: type[T]) -> Result[T]:
        result = self.call(
            "Runtime.evaluate",
            {
                "expression": expression,
                "returnByValue": True,
                "awaitPromise": True,
                "userGesture": True,
            },
        )
        if isinstance(result, Rejected):
            return result
        try:
            evaluation = _Evaluation.model_validate(result.value, strict=True)
            if evaluation.exceptionDetails is not None:
                return Rejected(
                    errno.EPROTO,
                    "JavaScript exception: " + json.dumps(evaluation.exceptionDetails),
                )
            return Ok(
                TypeAdapter(model).validate_python(evaluation.result.value, strict=True)
            )
        except ValidationError as error:
            return Rejected(errno.EPROTO, str(error))

    def events(self) -> tuple[Notification, ...]:
        events = tuple(self._events)
        self._events.clear()
        self._event_bytes = 0
        return events

    def _overflow(self, message: str) -> Rejected:
        # A missing event or forgotten response ID would make later replies
        # ambiguous. Surface the loss and retire this connection permanently.
        self._failure = Rejected(errno.ENOBUFS, message)
        self.close()
        return self._failure

    def close(self) -> None:
        self._lease.close()

    def __enter__(self) -> Cdp:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()
