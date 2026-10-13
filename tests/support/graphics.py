"""module: graphics; responsibility: observe the browser GPU configuration.

operations: get_info; entities: Info; tests: graphics_tests, browser acceptance.
"""

import errno

from pydantic import BaseModel, ValidationError

from tests.support.cdp import Cdp
from tests.support.outcome import Ok, Rejected, Result


class Info(BaseModel):
    skia: str
    renderer: str


class _Attributes(BaseModel):
    skiaBackendType: str
    glRenderer: str


class _Gpu(BaseModel):
    auxAttributes: _Attributes


class _Response(BaseModel):
    gpu: _Gpu


def get_info(control: Cdp) -> Result[Info]:
    result = control.call("SystemInfo.getInfo")
    if isinstance(result, Rejected):
        return result
    try:
        attributes = _Response.model_validate(
            result.value, strict=True
        ).gpu.auxAttributes
    except ValidationError as error:
        return Rejected(errno.EPROTO, str(error))
    return Ok(Info(skia=attributes.skiaBackendType, renderer=attributes.glRenderer))
