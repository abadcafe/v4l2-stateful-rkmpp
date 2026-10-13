"""module: uapi; responsibility: compare Python wire layouts with public C headers.

operations: verify; entities: field metadata;
tests: uapi_tests and sibling ABI module tests. No executable C client is produced.
"""

import ctypes
import errno
import re
import subprocess
from collections.abc import Mapping
from pathlib import Path

from pydantic import BaseModel, ValidationError

from tests.support.outcome import Ok, Rejected, Result


class _Field(BaseModel):
    offset: int
    size: int


def verify(
    wire_type: type[ctypes.Structure],
    c_type: str,
    fields: tuple[tuple[str, str], ...],
    constants: tuple[tuple[str, int], ...] = (),
) -> Result[None]:
    names = [c_type, *(name for _, name in fields), *(name for name, _ in constants)]
    if any(
        re.fullmatch(r"[a-zA-Z_]\w*(?:\.[a-zA-Z_]\w*)*", name) is None for name in names
    ):
        return Rejected(errno.EINVAL, "invalid public C identifier")
    checks = [
        f'_Static_assert(sizeof(struct {c_type}) == {ctypes.sizeof(wire_type)}, "size");',
        f'_Static_assert(_Alignof(struct {c_type}) == {ctypes.alignment(wire_type)}, "alignment");',
    ]
    attributes: Mapping[str, object] = wire_type.__dict__
    for python_name, c_name in fields:
        value = attributes.get(python_name)
        if value is None:
            return Rejected(errno.ENOENT, "unknown Python wire field: " + python_name)
        try:
            field = _Field.model_validate(value, from_attributes=True)
        except ValidationError:
            return Rejected(errno.EINVAL, "invalid ctypes wire field: " + python_name)
        checks.extend(
            [
                f'_Static_assert(offsetof(struct {c_type}, {c_name}) == {field.offset}, "offset {c_name}");',
                f'_Static_assert(sizeof(((struct {c_type} *)0)->{c_name}) == {field.size}, "field size {c_name}");',
            ]
        )
    checks.extend(
        f'_Static_assert({name} == {value}, "constant {name}");'
        for name, value in constants
    )
    source = "\n".join(
        [
            "#define _POSIX_C_SOURCE 200809L",
            "#include <stddef.h>",
            "#include <time.h>",
            "#include <sys/time.h>",
            "#include <linux/videodev2.h>",
            '#include "bridge_uapi.h"',
            *checks,
        ]
    )
    try:
        completed = subprocess.run(
            [
                "cc",
                "-x",
                "c",
                "-std=c11",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-fsyntax-only",
                "-I",
                str(Path(__file__).resolve().parents[2] / "kernel"),
                "-",
            ],
            input=source,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
    except OSError as error:
        return Rejected(error.errno or 0, "compile public C UAPI")
    except subprocess.TimeoutExpired:
        return Rejected(errno.ETIMEDOUT, "compile public C UAPI")
    return (
        Ok(None)
        if completed.returncode == 0
        else Rejected(errno.EINVAL, completed.stderr)
    )
