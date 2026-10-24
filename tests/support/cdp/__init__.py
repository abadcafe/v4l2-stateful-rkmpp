"""module: cdp; responsibility: exchange typed Chromium debugging messages.

operations: Cdp open/call/evaluate/events/close; entities: Cdp, Notification,
Payload; tests: cdp_tests (success, errors, timeout, event order).
"""

import errno
import json
import math
import time
from types import TracebackType

from pydantic import BaseModel, Field, JsonValue, TypeAdapter, ValidationError

from tests.support.cdp.transport import Transport
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
    result: _RemoteObject
    exceptionDetails: Payload | None = None


class Cdp:
    def __init__(self, connection: Transport) -> None:
        self._connection: Transport = connection
        self._sequence: int = 0
        self._expired: set[int] = set()
        self._events: list[Notification] = []
        self._event_bytes: int = 0
        self._failure: Rejected | None = None

    @classmethod
    def open(cls, url: str) -> Result[Cdp]:
        opened = Transport.open(url)
        return Ok(cls(opened.value)) if isinstance(opened, Ok) else opened

    def call(
        self, method: str, params: Payload | None = None, *, timeout: float = 10
    ) -> Result[Payload]:
        if not math.isfinite(timeout) or timeout <= 0:
            return Rejected(errno.EINVAL, "invalid CDP deadline")
        if self._failure is not None:
            return self._failure
        if len(self._expired) >= _MAX_EXPIRED:
            return self._retire(errno.ENOBUFS, "too many unanswered CDP commands")
        self._sequence += 1
        deadline = time.monotonic() + timeout
        wire = json.dumps(
            {"id": self._sequence, "method": method, "params": params or {}}
        )
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return Rejected(errno.ETIMEDOUT, "CDP " + method)
        sent = self._connection.send(wire, remaining)
        if isinstance(sent, Rejected):
            # A canceled write may already have transferred a partial frame.
            # It cannot be retried on the same protocol connection.
            return self._retire(
                sent.code,
                "CDP " + method if sent.code == errno.ETIMEDOUT else sent.operation,
            )
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                self._expired.add(self._sequence)
                return Rejected(errno.ETIMEDOUT, "CDP " + method)
            received = self._connection.receive(remaining)
            if isinstance(received, Rejected):
                if received.code == errno.ETIMEDOUT:
                    self._expired.add(self._sequence)
                    return Rejected(errno.ETIMEDOUT, "CDP " + method)
                return self._retire(received.code, received.operation)
            raw = received.value
            try:
                envelope = _Envelope.model_validate_json(raw, strict=True)
            except ValidationError as error:
                return self._retire(errno.EPROTO, str(error))
            if (
                envelope.id is not None
                and envelope.method is not None
                or envelope.result is not None
                and envelope.error is not None
                or envelope.id is None
                and (envelope.result is not None or envelope.error is not None)
            ):
                return self._retire(errno.EPROTO, "ambiguous CDP envelope")
            if envelope.id is None:
                if envelope.method is None:
                    return self._retire(
                        errno.EPROTO, "CDP response has neither id nor method"
                    )
                size = len(raw.encode()) if isinstance(raw, str) else len(raw)
                if (
                    len(self._events) >= _MAX_EVENTS
                    or self._event_bytes + size > _MAX_EVENT_BYTES
                ):
                    return self._retire(
                        errno.ENOBUFS,
                        "unconsumed CDP notifications exceed capacity",
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
                return self._retire(errno.EPROTO, "unexpected CDP response id")
            if envelope.error is not None:
                return Rejected(envelope.error.code, envelope.error.message)
            if envelope.result is None:
                return self._retire(errno.EPROTO, "CDP reply has no result or error")
            return Ok(envelope.result)

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

    def _retire(self, code: int, message: str) -> Rejected:
        # A missing event or forgotten response ID would make later replies
        # ambiguous. Surface the loss and retire this connection permanently.
        self._failure = Rejected(code, message)
        self.close()
        return self._failure

    def close(self) -> None:
        if self._failure is None:
            self._failure = Rejected(errno.ECONNRESET, "CDP connection closed")
        self._connection.close()

    def __enter__(self) -> Cdp:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()
