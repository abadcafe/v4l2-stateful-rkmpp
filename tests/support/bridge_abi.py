"""module: bridge_abi; responsibility: encode the published bridge UAPI.

operations: Message, Batch, Pool, Descriptor, Event, Reply;
entities: four wire structures and two enums; tests: kernel/test_layout, test_batch, test_pool.
"""

import ctypes
from enum import IntEnum
from typing import final

from tests.support.syscalls import command

VERSION = 2
MAX_BATCH = 16
MAX_BUFFERS = 32


class Event(IntEnum):
    PACKET = 1
    CAPTURE = 2
    RECYCLE = 3
    DRAIN = 4
    RESET = 5
    CLOSE = 6
    RESUME = 7


class Reply(IntEnum):
    LAYOUT = 32
    INPUT_DONE = 33
    FRAME = 34
    EOS = 35
    STOPPED = 36
    FAILED = 37


@final
class Message(ctypes.Structure):
    version: int = 0
    kind: int = 0
    epoch: int = 0
    index: int = 0
    flags: int = 0
    timestamp: int = 0
    bytesused: int = 0
    codec: int = 0
    width: int = 0
    height: int = 0
    stride: int = 0
    rows: int = 0
    size: int = 0
    format: int = 0
    count: int = 0
    generation: int = 0
    colorspace: int = 0
    xfer_func: int = 0
    ycbcr_enc: int = 0
    quantization: int = 0
    pool_id: int = 0
    reserved: ctypes.Array[ctypes.c_uint32] = (ctypes.c_uint32 * 3)()
    _fields_ = [
        ("version", ctypes.c_uint32),
        ("kind", ctypes.c_uint32),
        ("epoch", ctypes.c_uint64),
        ("index", ctypes.c_uint32),
        ("flags", ctypes.c_uint32),
        ("timestamp", ctypes.c_uint64),
        *[
            (field, ctypes.c_uint32)
            for field in (
                "bytesused",
                "codec",
                "width",
                "height",
                "stride",
                "rows",
                "size",
                "format",
                "count",
                "generation",
                "colorspace",
                "xfer_func",
                "ycbcr_enc",
                "quantization",
                "pool_id",
            )
        ],
        ("reserved", ctypes.c_uint32 * 3),
    ]


@final
class Batch(ctypes.Structure):
    version: int = 0
    reply_count: int = 0
    event_count: int = 0
    reserved: int = 0
    replies: ctypes.Array[Message] = (Message * 16)()
    events: ctypes.Array[Message] = (Message * 16)()
    results: ctypes.Array[ctypes.c_int32] = (ctypes.c_int32 * 16)()
    _fields_ = [
        ("version", ctypes.c_uint32),
        ("reply_count", ctypes.c_uint32),
        ("event_count", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32),
        ("replies", Message * MAX_BATCH),
        ("events", Message * MAX_BATCH),
        ("results", ctypes.c_int32 * MAX_BATCH),
    ]


@final
class Descriptor(ctypes.Structure):
    fd: int = 0
    size: int = 0
    _fields_ = [("fd", ctypes.c_int32), ("size", ctypes.c_uint32)]


@final
class Pool(ctypes.Structure):
    version: int = 0
    kind: int = 0
    epoch: int = 0
    pool_id: int = 0
    count: int = 0
    reserved: ctypes.Array[ctypes.c_uint32] = (ctypes.c_uint32 * 2)()
    buffers: ctypes.Array[Descriptor] = (Descriptor * 32)()
    _fields_ = [
        ("version", ctypes.c_uint32),
        ("kind", ctypes.c_uint32),
        ("epoch", ctypes.c_uint64),
        ("pool_id", ctypes.c_uint32),
        ("count", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32 * 2),
        ("buffers", Descriptor * MAX_BUFFERS),
    ]


ACCEPT = command(0, "R", 0x80)
EXCHANGE = command(3, "R", 0x84, ctypes.sizeof(Batch))
REGISTER_POOL = command(3, "R", 0x85, ctypes.sizeof(Pool))
