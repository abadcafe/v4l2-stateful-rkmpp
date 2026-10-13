"""module: window; responsibility: own a background Chromium window target.

operations: Window open/target/close; entities: Window;
tests: window_tests, browser/test_graphics.
The compositor decides focus; no activation or minimization is requested.
"""

import errno

from pydantic import BaseModel, Field, ValidationError

from tests.support.cdp import Cdp
from tests.support.outcome import Ok, Rejected, Result


class _Created(BaseModel):
    targetId: str = Field(min_length=1)


class _Closed(BaseModel):
    success: bool


class Window:
    def __init__(self, control: Cdp, target: str) -> None:
        self._control: Cdp = control
        self._target: str = target
        self._closed: bool = False

    @classmethod
    def open(cls, control: Cdp) -> Result[Window]:
        result = control.call(
            "Target.createTarget",
            {"url": "about:blank", "newWindow": True, "background": True},
        )
        if isinstance(result, Rejected):
            return result
        try:
            created = _Created.model_validate(result.value, strict=True)
        except ValidationError as error:
            return Rejected(errno.EPROTO, str(error))
        return Ok(cls(control, created.targetId))

    @property
    def target(self) -> str:
        return self._target

    def close(self) -> Result[None]:
        if self._closed:
            return Ok(None)
        result = self._control.call("Target.closeTarget", {"targetId": self._target})
        if isinstance(result, Rejected):
            return result
        try:
            closed = _Closed.model_validate(result.value, strict=True)
        except ValidationError as error:
            return Rejected(errno.EPROTO, str(error))
        if not closed.success:
            return Rejected(errno.EIO, "Chromium refused to close owned window")
        self._closed = True
        return Ok(None)
