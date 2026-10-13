"""module: control_abi; responsibility: encode public Linux V4L2 controls.

operations: Control, QueryControl, QueryMenu; entities: three wire structures;
tests: control_abi_tests, kernel/test_controls.
"""

import ctypes
from typing import final


@final
class Control(ctypes.Structure):
    id: int = 0
    value: int = 0
    _fields_ = [("id", ctypes.c_uint32), ("value", ctypes.c_int32)]


@final
class QueryControl(ctypes.Structure):
    id: int = 0
    kind: int = 0
    name: ctypes.Array[ctypes.c_ubyte] = (ctypes.c_ubyte * 32)()
    minimum: int = 0
    maximum: int = 0
    step: int = 0
    default: int = 0
    flags: int = 0
    _fields_ = [
        ("id", ctypes.c_uint32),
        ("kind", ctypes.c_uint32),
        ("name", ctypes.c_ubyte * 32),
        *[(name, ctypes.c_int32) for name in ("minimum", "maximum", "step", "default")],
        ("flags", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32 * 2),
    ]


@final
class QueryMenu(ctypes.Structure):
    id: int = 0
    index: int = 0
    name: ctypes.Array[ctypes.c_ubyte] = (ctypes.c_ubyte * 32)()
    _pack_ = 1
    # Python 3.14 requires an explicit packing layout; the Linux header
    # comparison verifies this payload's size, alignment and field offsets.
    _layout_ = "ms"
    _fields_ = [
        ("id", ctypes.c_uint32),
        ("index", ctypes.c_uint32),
        ("name", ctypes.c_ubyte * 32),
        ("reserved", ctypes.c_uint32),
    ]
