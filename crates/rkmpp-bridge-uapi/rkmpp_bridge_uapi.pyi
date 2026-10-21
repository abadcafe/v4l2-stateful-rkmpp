"""Typed public interface to the shared Rust/Python bridge UAPI."""

from enum import IntEnum, auto
from typing import ClassVar

VERSION: int
MAX_BATCH: int
MAX_BUFFERS: int
MIN_CAPTURE: int
ACCEPT: int
EXCHANGE: int
REGISTER_POOL: int

class Event(IntEnum):
    PACKET = auto()
    CAPTURE = auto()
    RECYCLE = auto()
    DRAIN = auto()
    RESET = auto()
    CLOSE = auto()
    RESUME = auto()

class Reply(IntEnum):
    LAYOUT = auto()
    INPUT_DONE = auto()
    FRAME = auto()
    EOS = auto()
    STOPPED = auto()
    FAILED = auto()
    DRAINING = auto()

class Message:
    CAPTURE_REUSE: ClassVar[int]
    FRAME_ERROR: ClassVar[int]
    RESET_CAPTURE_ONLY: ClassVar[int]
    RESET_ABORT_DRAIN: ClassVar[int]
    SIZE: ClassVar[int]
    ALIGN: ClassVar[int]
    version: int
    kind: int
    epoch: int
    index: int
    flags: int
    timestamp: int
    bytesused: int
    codec: int
    width: int
    height: int
    stride: int
    rows: int
    size: int
    format: int
    count: int
    generation: int
    colorspace: int
    xfer_func: int
    ycbcr_enc: int
    quantization: int
    pool_id: int
    data_offset: int
    crop_left: int
    crop_top: int
    coded_width: int
    coded_height: int
    field: int
    reserved: int
    def __init__(
        self,
        *,
        version: int = ...,
        kind: int = ...,
        epoch: int = ...,
        index: int = ...,
        flags: int = ...,
        timestamp: int = ...,
        bytesused: int = ...,
        codec: int = ...,
        width: int = ...,
        height: int = ...,
        stride: int = ...,
        rows: int = ...,
        size: int = ...,
        format: int = ...,
        count: int = ...,
        generation: int = ...,
        colorspace: int = ...,
        xfer_func: int = ...,
        ycbcr_enc: int = ...,
        quantization: int = ...,
        pool_id: int = ...,
        data_offset: int = ...,
        crop_left: int = ...,
        crop_top: int = ...,
        coded_width: int = ...,
        coded_height: int = ...,
        field: int = ...,
        reserved: int = ...,
    ) -> None: ...
    @staticmethod
    def decode(data: bytes) -> Message | None: ...
    def clone(self) -> Message: ...
    def __bytes__(self) -> bytes: ...

class Descriptor:
    SIZE: ClassVar[int]
    ALIGN: ClassVar[int]
    fd: int
    size: int
    def __init__(self, *, fd: int = ..., size: int = ...) -> None: ...
    @staticmethod
    def decode(data: bytes) -> Descriptor | None: ...
    def clone(self) -> Descriptor: ...
    def __bytes__(self) -> bytes: ...

class Pool:
    SIZE: ClassVar[int]
    ALIGN: ClassVar[int]
    version: int
    kind: int
    epoch: int
    pool_id: int
    count: int
    reserved: list[int]
    def __init__(
        self,
        *,
        version: int = ...,
        kind: int = ...,
        epoch: int = ...,
        pool_id: int = ...,
        count: int = ...,
        reserved: list[int] = ...,
    ) -> None: ...
    @staticmethod
    def decode(data: bytes) -> Pool | None: ...
    def clone(self) -> Pool: ...
    def __bytes__(self) -> bytes: ...
    def descriptor(self, index: int) -> Descriptor | None: ...
    def set_descriptor(self, index: int, value: Descriptor) -> bool: ...
    @staticmethod
    def descriptor_offset(index: int) -> int | None: ...
    def ioctl(self, fd: int, request: int) -> int: ...
    def ioctl_readonly(self, fd: int, request: int) -> int: ...

class Batch:
    SIZE: ClassVar[int]
    ALIGN: ClassVar[int]
    version: int
    reply_count: int
    event_count: int
    reserved: int
    def __init__(
        self,
        *,
        version: int = ...,
        reply_count: int = ...,
        event_count: int = ...,
        reserved: int = ...,
    ) -> None: ...
    @staticmethod
    def decode(data: bytes) -> Batch | None: ...
    def clone(self) -> Batch: ...
    def __bytes__(self) -> bytes: ...
    def result(self, index: int) -> int | None: ...
    def results(self, count: int) -> list[int] | None: ...
    def event(self, index: int) -> Message | None: ...
    def reply(self, index: int) -> Message | None: ...
    def set_event(self, index: int, value: Message) -> bool: ...
    def set_reply(self, index: int, value: Message) -> bool: ...
    def set_result(self, index: int, value: int) -> bool: ...
    def ioctl(self, fd: int, request: int) -> int: ...
    def ioctl_readonly(self, fd: int, request: int) -> int: ...
