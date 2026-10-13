"""module: mock_decode; responsibility: drive a kernel-facing decode transaction.

operations: play, Frame, Decoded; entities: Frame, Decoded, streaming state;
tests: service/test_decode, service reconfiguration/concurrency/lifetimes.
All image memory is real DMA-BUF; only bridge events and acknowledgements are simulated.
"""

import ctypes
import errno
import hashlib
from collections import deque
from contextlib import ExitStack
from pathlib import Path

from pydantic import BaseModel

from tests.support.bridge_abi import (
    EXCHANGE,
    REGISTER_POOL,
    VERSION,
    Batch,
    Descriptor,
    Event,
    Message,
    Pool,
    Reply,
)
from tests.support.dma_heap import Buffer
from tests.support.dmabuf import i420
from tests.support.mock_process import Process
from tests.support.mock_wire import Patch, Request
from tests.support.mock_wire import Reply as Response
from tests.support.outcome import Ok, Rejected, Result
from tests.support.stream import Codec, Packet, load


class Frame(BaseModel):
    digest: str
    width: int
    height: int
    timestamp: int
    epoch: int
    colorspace: int
    transfer: int
    ycbcr: int
    quantization: int


class Decoded(BaseModel):
    frames: tuple[Frame, ...]
    layouts: tuple[tuple[int, int], ...]
    eos: bool
    registrations: tuple[tuple[int, int], ...]
    maximum_reply_batch: int


class _Stream:
    def __init__(
        self,
        codec: Codec,
        packets: tuple[Packet, ...],
        repetitions: int,
        owned: ExitStack,
    ) -> None:
        self._codec: Codec = codec
        self._packets: tuple[Packet, ...] = packets
        self._owned: ExitStack = owned
        self._repetitions: int = repetitions
        self._completed: int = 0
        self._restarting: bool = False
        self._input_id: int = 1
        self._inputs: list[Buffer] = []
        self._capture: list[Buffer] = []
        self._retired: list[Buffer] = []
        self._events: deque[Message] = deque()
        self._next: int = 0
        self._epoch: int = 1
        self._generation: int = 0
        self._capture_id: int = 0
        self._layout: Message | None = None
        self._frames: list[Frame] = []
        self._layouts: list[tuple[int, int]] = []
        self._inflight: set[int] = set()
        self._available: set[int] = set()
        self._draining: bool = False
        self._replacing: bool = False
        self._eos: bool = False
        self._error: Rejected | None = None
        self._registrations: list[tuple[int, int]] = []
        self._maximum_reply_batch: int = 0

    def _event(self, kind: int, index: int = 0, **fields: int) -> Message:
        return Message(
            version=VERSION,
            kind=kind,
            epoch=self._epoch,
            generation=self._generation,
            pool_id=self._input_id if kind == Event.PACKET else self._capture_id,
            index=index,
            **fields,
        )

    def _fail(self, operation: str, code: int = errno.EPROTO) -> Response:
        self._error = Rejected(code, operation)
        return Response(code=code)

    def _input(self, index: int) -> Result[None]:
        if self._next == len(self._packets):
            return Ok(None)
        packet = self._packets[self._next]
        written = self._inputs[index].write(packet.data)
        if isinstance(written, Rejected):
            return written
        self._next += 1
        self._inflight.add(index)
        self._events.append(
            self._event(
                Event.PACKET,
                index,
                codec=self._codec.fourcc,
                bytesused=len(packet.data),
                timestamp=packet.timestamp * 1000,
            )
        )
        return Ok(None)

    def _prepare_capture(self) -> Result[None]:
        assert self._layout is not None
        self._capture = []
        for _ in range(32):
            buffer = Buffer.allocate(self._layout.size)
            if isinstance(buffer, Rejected):
                return buffer
            self._capture.append(self._owned.enter_context(buffer.value))
        self._available = set(range(len(self._capture)))
        self._events.append(self._event(Event.CAPTURE, count=len(self._capture)))
        self._events.extend(
            self._event(Event.RECYCLE, index) for index in self._available
        )
        return Ok(None)

    def _register(self, request: Request) -> Response:
        pool = Pool.from_buffer_copy(request.body)
        if (
            pool.version != VERSION
            or pool.epoch != self._epoch
            or (pool.kind == 10 and pool.pool_id != self._input_id)
            or (pool.kind == 9 and pool.pool_id != self._capture_id)
            or pool.kind not in (9, 10)
        ):
            return self._fail("invalid service pool registration")
        buffers = self._inputs if pool.kind == 10 else self._capture
        self._registrations.append((pool.kind, pool.pool_id))
        pool.count = len(buffers)
        for index, buffer in enumerate(buffers):
            descriptor = Descriptor.from_buffer(
                pool.buffers, index * ctypes.sizeof(Descriptor)
            )
            descriptor.fd = -1
            descriptor.size = buffer.size
        return Response(
            body=bytes(pool),
            fds=tuple(buffer.fd for buffer in buffers),
            patches=tuple(
                Patch(32 + index * 8, index) for index in range(len(buffers))
            ),
        )

    def _consume(self, message: Message) -> Result[None]:
        if message.version != VERSION or message.epoch != self._epoch:
            return Rejected(errno.EPROTO, "service reply version or epoch changed")
        if message.kind == Reply.INPUT_DONE:
            if message.pool_id != self._input_id or message.index not in self._inflight:
                return Rejected(errno.EPROTO, "service completed an unowned input")
            self._inflight.remove(message.index)
            return self._input(message.index)
        if message.kind == Reply.LAYOUT:
            if message.format != 1 or message.size <= 0:
                return Rejected(errno.EPROTO, "service published invalid NV12 storage")
            replacing = self._layout is not None
            self._layout = Message.from_buffer_copy(message)
            self._layouts.append((message.width, message.height))
            self._generation += 1
            self._capture_id += 1
            if replacing:
                self._events = deque(
                    event for event in self._events if event.kind != Event.RECYCLE
                )
                self._replacing = True
                self._events.append(self._event(Event.RESET, flags=9 | (1 << 16)))
                return Ok(None)
            return self._prepare_capture()
        if message.kind == Reply.STOPPED:
            if self._restarting:
                self._restarting = False
                self._next = 0
                self._draining = False
                self._inflight.clear()
                self._retired.extend(self._capture)
                self._capture = []
                self._layout = None
                self._input_id += 1
                self._events.append(self._event(Event.RESUME))
                for index in range(len(self._inputs)):
                    prepared = self._input(index)
                    if isinstance(prepared, Rejected):
                        return prepared
                return Ok(None)
            if not self._replacing:
                return Rejected(errno.EPROTO, "unsolicited stop acknowledgement")
            self._retired.extend(self._capture)
            self._capture = []
            self._replacing = False
            return self._prepare_capture()
        if message.kind == Reply.FRAME:
            layout = self._layout
            if (
                layout is None
                or message.pool_id != self._capture_id
                or message.generation != self._generation
                or message.index not in self._available
                or message.bytesused != layout.stride * layout.rows * 3 // 2
            ):
                return Rejected(
                    errno.EPROTO, "service completed an unowned capture slot"
                )
            self._available.remove(message.index)
            pixels = i420(
                self._capture[message.index].fd,
                layout.width,
                layout.height,
                layout.stride,
                layout.rows,
            )
            if isinstance(pixels, Rejected):
                return pixels
            self._frames.append(
                Frame(
                    digest=hashlib.sha256(pixels.value).hexdigest(),
                    width=layout.width,
                    height=layout.height,
                    timestamp=message.timestamp,
                    epoch=message.epoch,
                    colorspace=layout.colorspace,
                    transfer=layout.xfer_func,
                    ycbcr=layout.ycbcr_enc,
                    quantization=layout.quantization,
                )
            )
            self._available.add(message.index)
            self._events.append(self._event(Event.RECYCLE, message.index))
            return Ok(None)
        if message.kind == Reply.EOS:
            self._completed += 1
            self._events.clear()
            if self._completed < self._repetitions:
                self._epoch += 1
                self._restarting = True
                self._events.append(self._event(Event.RESET, flags=10))
                return Ok(None)
            self._eos = True
            self._events.append(self._event(Event.CLOSE))
            return Ok(None)
        return Rejected(errno.EPROTO, "unexpected service reply")

    def _handle(self, request: Request) -> Response:
        if request.command == REGISTER_POOL:
            return self._register(request)
        if request.command != EXCHANGE:
            return self._fail("service used unknown bridge command")
        batch = Batch.from_buffer_copy(request.body)
        if batch.version != VERSION or batch.reply_count > 16 or batch.reserved != 0:
            return self._fail("service emitted invalid exchange header")
        self._maximum_reply_batch = max(self._maximum_reply_batch, batch.reply_count)
        for index in range(batch.reply_count):
            message = Message.from_buffer_copy(
                batch.replies, index * ctypes.sizeof(Message)
            )
            consumed = self._consume(message)
            if isinstance(consumed, Rejected):
                return self._fail(consumed.operation, consumed.code)
            batch.results[index] = 0
        if (
            self._next == len(self._packets)
            and not self._inflight
            and not self._draining
            and not self._restarting
        ):
            self._draining = True
            self._events.append(self._event(Event.DRAIN))
        batch.event_count = min(16, len(self._events))
        for index in range(batch.event_count):
            batch.events[index] = self._events.popleft()
        return Response(body=bytes(batch))

    def run(self, process: Process) -> Result[Decoded]:
        size = max(4096, max(len(packet.data) for packet in self._packets))
        for index in range(min(8, len(self._packets))):
            allocation = Buffer.allocate(size)
            if isinstance(allocation, Rejected):
                return allocation
            self._inputs.append(self._owned.enter_context(allocation.value))
            prepared = self._input(index)
            if isinstance(prepared, Rejected):
                return prepared
        peer = process.session(self._handle)
        if isinstance(peer, Rejected):
            return peer
        completed = peer.value.wait(30)
        if isinstance(completed, Rejected):
            peer.value.close()
            return completed
        if self._error is not None:
            return self._error
        return Ok(
            Decoded(
                frames=tuple(self._frames),
                layouts=tuple(self._layouts),
                eos=self._eos,
                registrations=tuple(self._registrations),
                maximum_reply_batch=self._maximum_reply_batch,
            )
        )


def play(
    process: Process, codec: Codec, path: Path, repetitions: int = 1
) -> Result[Decoded]:
    if not 1 <= repetitions <= 32:
        return Rejected(errno.EINVAL, "invalid decode repetition count")
    packets = load(codec, path)
    if isinstance(packets, Rejected):
        return packets
    with ExitStack() as owned:
        return _Stream(codec, packets.value, repetitions, owned).run(process)
