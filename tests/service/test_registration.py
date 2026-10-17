"""module: test_registration; responsibility: specify process pool adoption failures.

operations: pytest cases; entities: InvalidField;
tests: malformed pools close every installed fd; stale/error registration is isolated.
"""

import ctypes
import errno
import time
from typing import Literal

import pytest

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
from tests.support.diagnostics import query
from tests.support.dma_heap import Buffer
from tests.support.mock_process import Process
from tests.support.mock_wire import Patch, Request
from tests.support.mock_wire import Reply as Response
from tests.support.outcome import accepted
from tests.support.stream import Codec

type InvalidField = Literal[
    "version",
    "kind",
    "epoch",
    "pool_id",
    "count_zero",
    "count_overflow",
    "reserved",
    "missing_fd",
    "duplicate_fd",
    "aliased_storage",
    "size_zero",
    "size_overflow",
]


@pytest.mark.parametrize(
    "field",
    [
        "version",
        "kind",
        "epoch",
        "pool_id",
        "count_zero",
        "count_overflow",
        "reserved",
        "missing_fd",
        "duplicate_fd",
        "aliased_storage",
        "size_zero",
        "size_overflow",
    ],
)
def test_register_invalid_pool_response_closes_all_transferred_descriptors(
    mock_service: Process,
    field: InvalidField,
) -> None:
    baseline = accepted(query(mock_service.diagnostics)).fds
    failed: list[int] = []
    with (
        accepted(Buffer.allocate(4096)) as first,
        accepted(Buffer.allocate(4096)) as second,
    ):

        def handle(request: Request) -> Response:
            if request.command == EXCHANGE:
                batch = Batch.from_buffer_copy(request.body)
                if batch.reply_count:
                    failed.extend(
                        Message.from_buffer_copy(
                            batch.replies, i * ctypes.sizeof(Message)
                        ).kind
                        for i in range(batch.reply_count)
                    )
                    return Response(code=errno.EPIPE)
                batch.event_count = 1
                batch.events[0] = Message(
                    version=VERSION,
                    kind=Event.PACKET,
                    epoch=1,
                    pool_id=1,
                    codec=Codec.H264.fourcc,
                    bytesused=16,
                )
                return Response(body=bytes(batch))
            assert request.command == REGISTER_POOL
            pool = Pool.from_buffer_copy(request.body)
            assert (pool.kind, pool.epoch, pool.pool_id) == (10, 1, 1)
            pool.count = 2
            for index in range(32):
                descriptor = Descriptor.from_buffer(
                    pool.buffers, index * ctypes.sizeof(Descriptor)
                )
                descriptor.fd, descriptor.size = -1, 4096 if index < 2 else 0
            fds = (first.fd, second.fd)
            patches = (Patch(32, 0), Patch(40, 1))
            if field == "version":
                pool.version += 1
            elif field == "kind":
                pool.kind = 9
            elif field == "epoch":
                pool.epoch += 1
            elif field == "pool_id":
                pool.pool_id += 1
            elif field == "reserved":
                pool.reserved[0] = 1
            elif field == "count_zero":
                pool.count, fds, patches = 0, (), ()
            elif field == "count_overflow":
                pool.count = 33
            elif field == "missing_fd":
                fds, patches = (first.fd,), (Patch(32, 0),)
            elif field == "duplicate_fd":
                fds, patches = (first.fd,), (Patch(32, 0), Patch(40, 0))
            elif field == "aliased_storage":
                # SCM_RIGHTS installs two distinct descriptor numbers, both
                # referring to the same allocation. Numeric fd checks alone
                # cannot enforce disjoint slot ownership.
                fds = (first.fd, first.fd)
            elif field == "size_zero":
                Descriptor.from_buffer(pool.buffers).size = 0
            else:
                Descriptor.from_buffer(pool.buffers).size = 8192
            return Response(body=bytes(pool), fds=fds, patches=patches)

        peer = accepted(mock_service.session(handle))
        accepted(peer.wait(5))
    assert failed == [Reply.FAILED]
    deadline = time.monotonic() + 5
    while accepted(query(mock_service.diagnostics)).fds != baseline:
        assert time.monotonic() < deadline
        time.sleep(0.01)
    assert mock_service.process.poll() is None


@pytest.mark.parametrize("code", [errno.ESTALE, errno.EPIPE, errno.EIO])
def test_register_errno_discards_stale_event_or_reports_failed(
    mock_service: Process,
    code: int,
) -> None:
    exchanges = 0
    failed: list[int] = []

    def handle(request: Request) -> Response:
        nonlocal exchanges
        if request.command == REGISTER_POOL:
            return Response(code=code)
        assert request.command == EXCHANGE
        exchanges += 1
        batch = Batch.from_buffer_copy(request.body)
        failed.extend(
            Message.from_buffer_copy(batch.replies, i * ctypes.sizeof(Message)).kind
            for i in range(batch.reply_count)
        )
        if exchanges != 1:
            return Response(code=errno.EPIPE)
        batch.event_count = 2
        batch.events[0] = Message(
            version=VERSION,
            kind=Event.PACKET,
            epoch=1,
            pool_id=1,
            codec=Codec.H264.fourcc,
            bytesused=16,
        )
        batch.events[1] = Message(version=VERSION, kind=Event.CLOSE, epoch=1)
        return Response(body=bytes(batch))

    peer = accepted(mock_service.session(handle))
    accepted(peer.wait(5))
    assert failed == ([Reply.FAILED] if code == errno.EIO else [])
    assert mock_service.process.poll() is None
