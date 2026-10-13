"""module: buffer_abi; responsibility: describe Linux multiplanar buffer payloads.

operations: Plane, VideoBuffer, ExportBuffer; entities: seven Linux ABI types;
tests: kernel/test_layout and kernel queue acceptance.
"""

import ctypes
from typing import final


@final
class Timeval(ctypes.Structure):
    seconds: int = 0
    microseconds: int = 0
    _fields_ = [("seconds", ctypes.c_long), ("microseconds", ctypes.c_long)]


@final
class PlaneMemory(ctypes.Union):
    offset: int = 0
    userptr: int = 0
    fd: int = 0
    _fields_ = [
        ("offset", ctypes.c_uint32),
        ("userptr", ctypes.c_ulong),
        ("fd", ctypes.c_int32),
    ]


@final
class Plane(ctypes.Structure):
    bytesused: int = 0
    length: int = 0
    memory: PlaneMemory = PlaneMemory()
    data_offset: int = 0
    reserved: ctypes.Array[ctypes.c_uint32] = (ctypes.c_uint32 * 11)()
    _fields_ = [
        ("bytesused", ctypes.c_uint32),
        ("length", ctypes.c_uint32),
        ("memory", PlaneMemory),
        ("data_offset", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32 * 11),
    ]


@final
class Timecode(ctypes.Structure):
    _fields_ = [("raw", ctypes.c_uint32 * 4)]


@final
class BufferMemory(ctypes.Union):
    planes: int | None = None
    offset: int = 0
    fd: int = 0
    _fields_ = [
        ("offset", ctypes.c_uint32),
        ("planes", ctypes.c_void_p),
        ("fd", ctypes.c_int32),
    ]


@final
class VideoBuffer(ctypes.Structure):
    index: int = 0
    kind: int = 0
    bytesused: int = 0
    flags: int = 0
    field: int = 0
    timestamp: Timeval = Timeval()
    timecode: Timecode = Timecode()
    sequence: int = 0
    memory: int = 0
    location: BufferMemory = BufferMemory()
    length: int = 0
    reserved2: int = 0
    request_fd: int = 0
    _fields_ = [
        *[
            (field, ctypes.c_uint32)
            for field in ("index", "kind", "bytesused", "flags", "field")
        ],
        ("timestamp", Timeval),
        ("timecode", Timecode),
        ("sequence", ctypes.c_uint32),
        ("memory", ctypes.c_uint32),
        ("location", BufferMemory),
        ("length", ctypes.c_uint32),
        ("reserved2", ctypes.c_uint32),
        ("request_fd", ctypes.c_int32),
    ]


@final
class ExportBuffer(ctypes.Structure):
    kind: int = 0
    index: int = 0
    plane: int = 0
    flags: int = 0
    fd: int = 0
    _fields_ = [
        ("kind", ctypes.c_uint32),
        ("index", ctypes.c_uint32),
        ("plane", ctypes.c_uint32),
        ("flags", ctypes.c_uint32),
        ("fd", ctypes.c_int32),
        ("reserved", ctypes.c_uint32 * 11),
    ]
