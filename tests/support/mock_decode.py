"""module: mock_decode; responsibility: drive a kernel-facing decode transaction.

operations: play, Frame, Decoded; entities: Frame, Decoded, streaming state;
tests: service/test_decode, service reconfiguration/concurrency/lifetimes.
All image memory is real DMA-BUF; only bridge events and acknowledgements are simulated.
"""

import errno
import hashlib
from collections import deque
from contextlib import ExitStack
from pathlib import Path

from pydantic import BaseModel
from rkmpp_bridge_uapi import (
    EXCHANGE,
    REGISTER_POOL,
    VERSION,
    Batch,
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
from tests.support.outcome import Ok, Rejected, Result, present
from tests.support.stream import Codec, Packet, load


class Frame(BaseModel):
    flags: int
    field: int
    digest: str
    width: int
    height: int
    timestamp: int
    epoch: int
    colorspace: int
    transfer: int
    ycbcr: int
    quantization: int


class InputCompletion(BaseModel):
    index: int
    flags: int
    timestamp: int


class DecodeError(BaseModel):
    timestamp: int
    flags: int


class Decoded(BaseModel):
    frames: tuple[Frame, ...]
    layouts: tuple[tuple[int, int], ...]
    capture_minimums: tuple[int, ...]
    eos: bool
    drain_frames: tuple[int, ...]
    registrations: tuple[tuple[int, int], ...]
    maximum_reply_batch: int
    reply_order: tuple[int, ...]
    input_completions: tuple[InputCompletion, ...]
    errors: tuple[DecodeError, ...]


class _Stream:
    def __init__(
        self,
        codec: Codec,
        packets: tuple[Packet, ...],
        repetitions: int,
        owned: ExitStack,
        initial_capture_size: int,
        reuse_capture: bool,
        drain_at: int,
        capture_restart: bool,
        input_count: int,
        natural_eos: bool,
        restart_at_frame: int,
        abort_drain: bool,
        natural_sequences: int,
        capture_count: int,
        sources: tuple[int, ...],
        capture_delay: int,
    ) -> None:
        self._capture_delay: int = capture_delay
        self._held_capture: deque[int] = deque()
        self._natural_remaining: int = natural_sequences
        self._capture_count: int = capture_count
        self._sources: tuple[int, ...] = sources
        self._abort_drain: bool = abort_drain
        self._codec: Codec = codec
        self._natural_eos: bool = natural_eos
        self._restart_at_frame: int = restart_at_frame
        self._ordinary_restart: bool = False
        self._input_count: int = input_count
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
        self._capture_minimums: list[int] = []
        self._inflight: dict[int, int] = {}
        self._available: dict[int, int] = {}
        self._capture_serial: int = 0
        self._reset_serial: int = 0
        self._reset_pending: int | None = None
        self._draining: bool = False
        self._replacing: bool = False
        self._eos: bool = False
        self._drain_frames: list[int] = []
        self._error: Rejected | None = None
        self._registrations: list[tuple[int, int]] = []
        self._maximum_reply_batch: int = 0
        self._initial_capture_size: int = initial_capture_size
        self._reuse_capture: bool = reuse_capture
        self._drain_at: int = drain_at
        self._capture_restart: bool = capture_restart
        self._resume_pending: bool = False
        self._reply_order: list[int] = []
        self._input_completions: list[InputCompletion] = []
        self._input_timestamps: dict[int, int] = {}
        self._errors: list[DecodeError] = []

    def _event(self, kind: int, index: int = 0, **fields: int) -> Message:
        if kind == Event.RESET:
            assert self._reset_pending is None
            self._reset_serial += 1
            self._reset_pending = self._reset_serial
            fields["count"] = self._reset_serial
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
        if self._next == len(self._packets) or (
            self._drain_at and self._next == self._drain_at
        ):
            return Ok(None)
        packet = self._packets[self._next]
        written = self._inputs[index].write(packet.data)
        if isinstance(written, Rejected):
            return written
        self._next += 1
        self._inflight[index] = self._next
        self._input_timestamps[index] = packet.timestamp * 1000
        self._events.append(
            self._event(
                Event.PACKET,
                index,
                codec=self._codec.fourcc,
                bytesused=len(packet.data),
                count=self._next,
                timestamp=packet.timestamp * 1000,
                flags=self._sources[self._next - 1],
            )
        )
        return Ok(None)

    def _recycle(self, index: int) -> Message:
        self._capture_serial += 1
        self._available[index] = self._capture_serial
        return self._event(Event.RECYCLE, index, count=self._capture_serial)

    def _prepare_capture(self, size: int) -> Result[None]:
        self._held_capture.clear()
        self._capture = []
        minimum = self._layout.count if self._layout is not None else 2
        for _ in range(max(self._capture_count, minimum)):
            buffer = Buffer.allocate(size)
            if isinstance(buffer, Rejected):
                return buffer
            self._capture.append(self._owned.enter_context(buffer.value))
        self._available.clear()
        self._events.append(self._event(Event.CAPTURE, count=len(self._capture)))
        self._events.extend(self._recycle(index) for index in range(len(self._capture)))
        return Ok(None)

    def _register(self, request: Request) -> Response:
        pool = present(Pool.decode(request.body))
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
            descriptor = present(pool.descriptor(index))
            descriptor.fd = -1
            descriptor.size = buffer.size
            assert pool.set_descriptor(index, descriptor)
        return Response(
            body=bytes(pool),
            fds=tuple(buffer.fd for buffer in buffers),
            patches=tuple(
                Patch(present(Pool.descriptor_offset(index)), index)
                for index in range(len(buffers))
            ),
        )

    def _consume(self, message: Message) -> Result[None]:
        if message.version != VERSION or message.epoch != self._epoch:
            return Rejected(errno.EPROTO, "service reply version or epoch changed")
        self._reply_order.append(message.kind)
        if message.kind == Reply.INPUT_DONE:
            if message.pool_id != self._input_id or message.count != self._inflight.get(
                message.index
            ):
                return Rejected(errno.EPROTO, "service completed an unowned input")
            _ = self._inflight.pop(message.index)
            self._input_completions.append(
                InputCompletion(
                    index=message.index,
                    flags=message.flags,
                    timestamp=self._input_timestamps.pop(message.index),
                )
            )
            return self._input(message.index)
        if message.kind == Reply.LAYOUT:
            if message.format != 1 or message.size <= 0:
                return Rejected(errno.EPROTO, "service published invalid NV12 storage")
            replacing = bool(self._capture)
            self._layout = message.clone()
            self._layouts.append((message.width, message.height))
            self._capture_minimums.append(message.count)
            self._generation += 1
            if (
                replacing
                and self._reuse_capture
                and len(self._capture) >= message.count
                and all(buffer.size >= message.size for buffer in self._capture)
            ):
                self._events = deque(
                    event for event in self._events if event.kind != Event.RECYCLE
                )
                self._events.append(
                    self._event(Event.CAPTURE, count=len(self._capture), flags=1)
                )
                self._events.extend(
                    self._event(Event.RECYCLE, index, count=token)
                    for index, token in self._available.items()
                )
                return Ok(None)
            self._capture_id += 1
            if replacing:
                self._events = deque(
                    event for event in self._events if event.kind != Event.RECYCLE
                )
                self._replacing = True
                self._events.append(self._event(Event.RESET, flags=(1 << 16)))
                return Ok(None)
            return self._prepare_capture(message.size)
        if message.kind == Reply.STOPPED:
            if self._reset_pending is None or message.count != self._reset_pending:
                return Rejected(errno.EPROTO, "stop transaction identity changed")
            self._reset_pending = None
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
            if self._ordinary_restart:
                self._ordinary_restart = False
                self._capture_id += 1
            self._retired.extend(self._capture)
            self._capture = []
            self._replacing = False
            assert self._layout is not None
            prepared = self._prepare_capture(self._layout.size)
            if isinstance(prepared, Rejected):
                return prepared
            if self._resume_pending:
                self._resume_pending = False
                return self._resume()
            return Ok(None)
        if message.kind == Reply.FRAME:
            layout = self._layout
            if (
                layout is None
                or message.pool_id != self._capture_id
                or message.generation != self._generation
                or message.count != self._available.get(message.index)
                or (
                    message.bytesused != layout.stride * layout.rows * 3 // 2
                    and not (message.bytesused == 0 and message.flags & 2)
                )
            ):
                return Rejected(
                    errno.EPROTO, "service completed an unowned capture slot"
                )
            _ = self._available.pop(message.index)
            if message.bytesused == 0:
                self._errors.append(
                    DecodeError(timestamp=message.timestamp, flags=message.flags)
                )
                self._events.append(self._recycle(message.index))
                return Ok(None)
            pixels = i420(
                self._capture[message.index].fd,
                layout.width,
                layout.height,
                layout.stride,
                layout.rows,
                left=layout.crop_left,
                top=layout.crop_top,
            )
            if isinstance(pixels, Rejected):
                return pixels
            self._frames.append(
                Frame(
                    flags=message.flags,
                    field=message.field,
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
            if self._draining and self._abort_drain:
                self._abort_drain = False
                self._drain_at = 0
                self._draining = False
                self._ordinary_restart = True
                self._replacing = True
                self._resume_pending = True
                self._events = deque(
                    event for event in self._events if event.kind != Event.RECYCLE
                )
                self._events.append(
                    self._event(Event.RESET, flags=(1 << 16) | (1 << 17))
                )
            elif len(self._frames) == self._restart_at_frame:
                self._ordinary_restart = True
                self._replacing = True
                self._events = deque(
                    event for event in self._events if event.kind != Event.RECYCLE
                )
                self._events.append(self._event(Event.RESET, flags=(1 << 16)))
            elif not self._ordinary_restart:
                self._held_capture.append(message.index)
                if len(self._held_capture) > self._capture_delay:
                    index = self._held_capture.popleft()
                    self._events.append(self._recycle(index))
            return Ok(None)
        if message.kind == Reply.DRAINING:
            self._draining = True
            return Ok(None)
        if message.kind == Reply.EOS:
            self._drain_frames.append(len(self._frames))
            if self._ordinary_restart and self._resume_pending:
                return Ok(None)
            if self._drain_at:
                self._drain_at = 0
                self._draining = False
                if self._capture_restart:
                    self._events.clear()
                    self._capture_id += 1
                    self._replacing = True
                    self._resume_pending = True
                    self._events.append(
                        self._event(Event.RESET, flags=(1 << 16) | (1 << 17))
                    )
                    return Ok(None)
                return self._resume()
            if self._natural_remaining > 1:
                self._natural_remaining -= 1
                self._events.appendleft(self._event(Event.RESUME))
                return Ok(None)
            self._completed += 1
            self._events.clear()
            if self._completed < self._repetitions:
                self._epoch += 1
                self._restarting = True
                self._events.append(self._event(Event.RESET, flags=0))
                return Ok(None)
            self._eos = True
            self._events.append(self._event(Event.CLOSE))
            return Ok(None)
        return Rejected(errno.EPROTO, "unexpected service reply")

    def _resume(self) -> Result[None]:
        self._events.append(self._event(Event.RESUME))
        for index in range(len(self._inputs)):
            if index not in self._inflight:
                prepared = self._input(index)
                if isinstance(prepared, Rejected):
                    return prepared
        return Ok(None)

    def _handle(self, request: Request) -> Response:
        if request.command == REGISTER_POOL:
            return self._register(request)
        if request.command != EXCHANGE:
            return self._fail("service used unknown bridge command")
        batch = present(Batch.decode(request.body))
        if batch.version != VERSION or batch.reply_count > 16 or batch.reserved != 0:
            return self._fail("service emitted invalid exchange header")
        self._maximum_reply_batch = max(self._maximum_reply_batch, batch.reply_count)
        for index in range(batch.reply_count):
            message = present(batch.reply(index))
            consumed = self._consume(message)
            if isinstance(consumed, Rejected):
                return self._fail(consumed.operation, consumed.code)
            assert batch.set_result(index, 0)
        if (
            (
                self._next == len(self._packets)
                or (self._drain_at and self._next == self._drain_at)
            )
            and not self._natural_eos
            and not self._draining
            and not self._restarting
        ):
            self._draining = True
            self._events.append(self._event(Event.DRAIN))
        batch.event_count = min(16, len(self._events))
        for index in range(batch.event_count):
            assert batch.set_event(index, self._events.popleft())
        return Response(body=bytes(batch))

    def run(self, process: Process) -> Result[Decoded]:
        if self._initial_capture_size:
            self._capture_id = 1
            prepared = self._prepare_capture(self._initial_capture_size)
            if isinstance(prepared, Rejected):
                return prepared
        size = max(4096, max(len(packet.data) for packet in self._packets))
        for index in range(min(self._input_count, len(self._packets))):
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
                capture_minimums=tuple(self._capture_minimums),
                eos=self._eos,
                drain_frames=tuple(self._drain_frames),
                registrations=tuple(self._registrations),
                maximum_reply_batch=self._maximum_reply_batch,
                reply_order=tuple(self._reply_order),
                input_completions=tuple(self._input_completions),
                errors=tuple(self._errors),
            )
        )


def play(
    process: Process,
    codec: Codec,
    path: Path,
    repetitions: int = 1,
    *,
    initial_capture_size: int = 0,
    reuse_capture: bool = False,
    drain_at: int = 0,
    capture_restart: bool = False,
    submissions: tuple[Packet, ...] | None = None,
    input_count: int = 8,
    natural_eos: bool = False,
    restart_at_frame: int = 0,
    timestamps: tuple[int, ...] = (),
    abort_drain: bool = False,
    natural_sequences: int = 1,
    capture_count: int = 2,
    sources: tuple[int, ...] = (),
    capture_delay: int = 0,
) -> Result[Decoded]:
    if not 1 <= repetitions <= 32:
        return Rejected(errno.EINVAL, "invalid decode repetition count")
    if submissions is None:
        packets = load(codec, path)
        if isinstance(packets, Rejected):
            return packets
        chunks = packets.value
    else:
        chunks = submissions
    if not chunks:
        return Rejected(errno.EINVAL, "no input submissions")
    if timestamps:
        if len(timestamps) != len(chunks):
            return Rejected(errno.EINVAL, "timestamp count does not match packets")
        chunks = tuple(
            Packet(data=p.data, timestamp=t)
            for p, t in zip(chunks, timestamps, strict=True)
        )
    if not 2 <= capture_count <= 32:
        return Rejected(errno.EINVAL, "invalid capture buffer count")
    if not 0 <= capture_delay < capture_count:
        return Rejected(errno.EINVAL, "invalid capture return delay")
    if not sources:
        sources = (0,) * len(chunks)
    if len(sources) != len(chunks) or any(s not in (0, 0x10000) for s in sources):
        return Rejected(errno.EINVAL, "invalid timestamp sources")
    if not 1 <= input_count <= 32:
        return Rejected(errno.EINVAL, "invalid input buffer count")
    with ExitStack() as owned:
        return _Stream(
            codec,
            chunks,
            repetitions,
            owned,
            initial_capture_size,
            reuse_capture,
            drain_at,
            capture_restart,
            input_count,
            natural_eos,
            restart_at_frame,
            abort_drain,
            natural_sequences,
            capture_count,
            sources,
            capture_delay,
        ).run(process)
