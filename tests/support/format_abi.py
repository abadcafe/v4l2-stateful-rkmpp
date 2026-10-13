"""module: format_abi; responsibility: encode Linux video format payloads.

operations: Format, RequestBuffers, Selection, Capability;
entities: seven Linux ABI types; tests: kernel/test_layout and kernel/test_formats.
"""

import ctypes
from typing import final


@final
class FormatUnion(ctypes.Union):
    data: ctypes.Array[ctypes.c_ubyte] = (ctypes.c_ubyte * 200)()
    _fields_ = [("data", ctypes.c_ubyte * 200), ("alignment", ctypes.c_uint64)]


@final
class Format(ctypes.Structure):
    kind: int = 0
    fmt: FormatUnion = FormatUnion()
    _fields_ = [("kind", ctypes.c_uint32), ("fmt", FormatUnion)]


@final
class RequestBuffers(ctypes.Structure):
    count: int = 0
    kind: int = 0
    memory: int = 0
    capabilities: int = 0
    flags: int = 0
    _fields_ = [
        ("count", ctypes.c_uint32),
        ("kind", ctypes.c_uint32),
        ("memory", ctypes.c_uint32),
        ("capabilities", ctypes.c_uint32),
        ("flags", ctypes.c_ubyte),
        ("reserved", ctypes.c_ubyte * 3),
    ]


@final
class Rect(ctypes.Structure):
    left: int = 0
    top: int = 0
    width: int = 0
    height: int = 0
    _fields_ = [
        ("left", ctypes.c_int32),
        ("top", ctypes.c_int32),
        ("width", ctypes.c_uint32),
        ("height", ctypes.c_uint32),
    ]


@final
class Selection(ctypes.Structure):
    kind: int = 0
    target: int = 0
    flags: int = 0
    rect: Rect = Rect()
    _fields_ = [
        ("kind", ctypes.c_uint32),
        ("target", ctypes.c_uint32),
        ("flags", ctypes.c_uint32),
        ("rect", Rect),
        ("reserved", ctypes.c_uint32 * 9),
    ]


@final
class Capability(ctypes.Structure):
    driver: ctypes.Array[ctypes.c_ubyte] = (ctypes.c_ubyte * 16)()
    card: ctypes.Array[ctypes.c_ubyte] = (ctypes.c_ubyte * 32)()
    bus_info: ctypes.Array[ctypes.c_ubyte] = (ctypes.c_ubyte * 32)()
    device_caps: int = 0
    _fields_ = [
        ("driver", ctypes.c_ubyte * 16),
        ("card", ctypes.c_ubyte * 32),
        ("bus_info", ctypes.c_ubyte * 32),
        ("version", ctypes.c_uint32),
        ("capabilities", ctypes.c_uint32),
        ("device_caps", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32 * 3),
    ]


@final
class FormatDescription(ctypes.Structure):
    index: int = 0
    kind: int = 0
    flags: int = 0
    pixelformat: int = 0
    _fields_ = [
        ("index", ctypes.c_uint32),
        ("kind", ctypes.c_uint32),
        ("flags", ctypes.c_uint32),
        ("description", ctypes.c_ubyte * 32),
        ("pixelformat", ctypes.c_uint32),
        ("mbus_code", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32 * 3),
    ]
