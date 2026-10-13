"""module: queue; responsibility: manage a V4L2 streaming queue through public ioctls.

operations: Queue request/query/queue/dequeue/export/map/stream_on/stream_off;
entities: Queue, QueueType, BufferInfo; tests: queue_tests and kernel/service acceptance.
"""

import ctypes
import mmap
import os
from enum import IntEnum

from pydantic import BaseModel

from tests.support.buffer_abi import ExportBuffer, Plane, VideoBuffer
from tests.support.event_abi import Scalar
from tests.support.format_abi import RequestBuffers
from tests.support.outcome import Ok, Rejected, Result
from tests.support.syscalls import OwnedFd, command, ioctl


class QueueType(IntEnum):
    CAPTURE = 9
    OUTPUT = 10


class BufferInfo(BaseModel):
    index: int
    flags: int
    sequence: int
    timestamp: int
    bytesused: tuple[int, ...]
    lengths: tuple[int, ...]
    offsets: tuple[int, ...]


class Queue:
    def __init__(self, fd: int, kind: QueueType) -> None:
        self._fd: int = fd
        self._kind: QueueType = kind
        self._planes: int = 1

    def request(self, count: int) -> Result[int]:
        payload = RequestBuffers(count=count, kind=self._kind, memory=1)
        result = ioctl(self._fd, command(3, "V", 8, ctypes.sizeof(payload)), payload)
        return Ok(payload.count) if isinstance(result, Ok) else result

    def _buffer(self, index: int = 0) -> tuple[VideoBuffer, ctypes.Array[Plane]]:
        planes = (Plane * self._planes)()
        payload = VideoBuffer(
            index=index, kind=self._kind, memory=1, length=self._planes
        )
        payload.location.planes = ctypes.addressof(planes)
        return payload, planes

    def _describe(
        self, payload: VideoBuffer, planes: ctypes.Array[Plane]
    ) -> BufferInfo:
        records = [
            Plane.from_buffer(planes, i * ctypes.sizeof(Plane))
            for i in range(len(planes))
        ]
        return BufferInfo(
            index=payload.index,
            flags=payload.flags,
            sequence=payload.sequence,
            timestamp=payload.timestamp.seconds * 1000000
            + payload.timestamp.microseconds,
            bytesused=tuple(p.bytesused for p in records),
            lengths=tuple(p.length for p in records),
            offsets=tuple(p.memory.offset for p in records),
        )

    def query(self, index: int) -> Result[BufferInfo]:
        payload, planes = self._buffer(index)
        result = ioctl(self._fd, command(3, "V", 9, ctypes.sizeof(payload)), payload)
        return Ok(self._describe(payload, planes)) if isinstance(result, Ok) else result

    def queue(self, index: int, bytesused: int = 0, timestamp: int = 0) -> Result[None]:
        payload, planes = self._buffer(index)
        Plane.from_buffer(planes).bytesused = bytesused
        payload.timestamp.seconds, payload.timestamp.microseconds = divmod(
            timestamp, 1000000
        )
        result = ioctl(self._fd, command(3, "V", 15, ctypes.sizeof(payload)), payload)
        return Ok(None) if isinstance(result, Ok) else result

    def dequeue(self) -> Result[BufferInfo]:
        payload, planes = self._buffer()
        result = ioctl(self._fd, command(3, "V", 17, ctypes.sizeof(payload)), payload)
        return Ok(self._describe(payload, planes)) if isinstance(result, Ok) else result

    def export(self, index: int, plane: int = 0) -> Result[OwnedFd]:
        payload = ExportBuffer(
            kind=self._kind, index=index, plane=plane, flags=os.O_CLOEXEC
        )
        result = ioctl(self._fd, command(3, "V", 16, ctypes.sizeof(payload)), payload)
        return Ok(OwnedFd(payload.fd)) if isinstance(result, Ok) else result

    def map(self, index: int) -> Result[mmap.mmap]:
        info = self.query(index)
        if isinstance(info, Rejected):
            return info
        try:
            mapping = mmap.mmap(
                self._fd,
                info.value.lengths[0],
                flags=mmap.MAP_SHARED,
                prot=mmap.PROT_READ | mmap.PROT_WRITE,
                offset=info.value.offsets[0],
            )
        except OSError as error:
            return Rejected(error.errno or 0, "mmap V4L2 queue")
        return Ok(mapping)

    def stream_on(self) -> Result[None]:
        payload = Scalar(value=self._kind)
        result = ioctl(self._fd, command(1, "V", 18, 4), payload)
        return Ok(None) if isinstance(result, Ok) else result

    def stream_off(self) -> Result[None]:
        payload = Scalar(value=self._kind)
        result = ioctl(self._fd, command(1, "V", 19, 4), payload)
        return Ok(None) if isinstance(result, Ok) else result
