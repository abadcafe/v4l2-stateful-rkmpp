"""module: event_abi; responsibility: encode V4L2 streaming control payloads.

operations: Subscription, Event, DecoderCommand, Scalar;
entities: seven Linux ABI types; tests: event_abi_tests, kernel/test_layout.
"""

import ctypes
from typing import final


@final
class EventUnion(ctypes.Union):
    changes: int = 0
    _fields_ = [
        ("data", ctypes.c_ubyte * 64),
        ("changes", ctypes.c_uint32),
        ("alignment", ctypes.c_uint64),
    ]


@final
class Timespec(ctypes.Structure):
    _fields_ = [("seconds", ctypes.c_long), ("nanoseconds", ctypes.c_long)]


@final
class Event(ctypes.Structure):
    kind: int = 0
    payload: EventUnion = EventUnion()
    _fields_ = [
        ("kind", ctypes.c_uint32),
        ("payload", EventUnion),
        ("pending", ctypes.c_uint32),
        ("sequence", ctypes.c_uint32),
        ("timestamp", Timespec),
        ("id", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32 * 8),
    ]


@final
class Subscription(ctypes.Structure):
    kind: int = 0
    _fields_ = [
        ("kind", ctypes.c_uint32),
        ("id", ctypes.c_uint32),
        ("flags", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32 * 5),
    ]


@final
class DecoderData(ctypes.Union):
    _fields_ = [("data", ctypes.c_uint32 * 16), ("alignment", ctypes.c_uint64)]


@final
class DecoderCommand(ctypes.Structure):
    command: int = 0
    _fields_ = [
        ("command", ctypes.c_uint32),
        ("flags", ctypes.c_uint32),
        ("data", DecoderData),
    ]


@final
class Scalar(ctypes.Structure):
    value: int = 0
    _fields_ = [("value", ctypes.c_uint32)]
