"""module: test_recovery; responsibility: specify process termination and restart.

operations: pytest cases; entities: none;
tests: SIGKILL during MPP work, clean idle/active SIGINT and SIGTERM.
The kernel endpoint is simulated; signals, workers and hardware are real.
"""

import ctypes
import signal
import threading
from pathlib import Path

import pytest

from tests.service.conftest import MockFactory
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
from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.mock_wire import Patch, Request
from tests.support.mock_wire import Reply as Response
from tests.support.outcome import accepted
from tests.support.stream import Codec


def decoding(process: Process, buffer: Buffer, size: int) -> threading.Event:
    initialized = threading.Event()
    exchanges = 0

    def handle(request: Request) -> Response:
        nonlocal exchanges
        if request.command == REGISTER_POOL:
            pool = Pool.from_buffer_copy(request.body)
            pool.count = 1
            descriptor = Descriptor.from_buffer(pool.buffers)
            descriptor.fd, descriptor.size = -1, buffer.size
            return Response(body=bytes(pool), fds=(buffer.fd,), patches=(Patch(32, 0),))
        assert request.command == EXCHANGE
        exchanges += 1
        batch = Batch.from_buffer_copy(request.body)
        for index in range(batch.reply_count):
            message = Message.from_buffer_copy(
                batch.replies, index * ctypes.sizeof(Message)
            )
            assert message.kind in (Reply.INPUT_DONE, Reply.LAYOUT)
            if message.kind == Reply.LAYOUT:
                assert (message.width, message.height, message.format) == (256, 128, 1)
                initialized.set()
        batch.event_count = int(exchanges == 1)
        if batch.event_count:
            batch.events[0] = Message(
                version=VERSION,
                kind=Event.PACKET,
                epoch=1,
                pool_id=1,
                codec=Codec.H264.fourcc,
                bytesused=size,
            )
        return Response(body=bytes(batch))

    _ = accepted(process.session(handle))
    return initialized


def test_service_sigkill_during_real_mpp_decode_allows_fresh_daemon_restart(
    mock_factory: MockFactory,
    artifacts: Path,
) -> None:
    process = mock_factory()
    data = (artifacts / "h264-yuv420p.h264").read_bytes()
    with accepted(Buffer.allocate(max(4096, len(data)))) as buffer:
        accepted(buffer.write(data))
        assert decoding(process, buffer, len(data)).wait(5)
        process.process.kill()
        assert process.process.wait(timeout=5) == -signal.SIGKILL
    restarted = mock_factory()
    result = accepted(play(restarted, Codec.VP9, artifacts / "vp9-yuv420p10le.ivf"))
    assert result.eos and len(result.frames) == 36
    restarted.process.terminate()
    assert restarted.process.wait(timeout=5) == 0


@pytest.mark.parametrize("number", [signal.SIGINT, signal.SIGTERM])
def test_service_signal_idle_broker_exits_cleanly(
    mock_service: Process, number: int
) -> None:
    mock_service.process.send_signal(number)
    assert mock_service.process.wait(timeout=5) == 0


@pytest.mark.parametrize("number", [signal.SIGINT, signal.SIGTERM])
def test_service_signal_active_mpp_worker_exits_and_releases_session(
    mock_service: Process,
    artifacts: Path,
    number: int,
) -> None:
    data = (artifacts / "h264-yuv420p.h264").read_bytes()
    with accepted(Buffer.allocate(max(4096, len(data)))) as buffer:
        accepted(buffer.write(data))
        assert decoding(mock_service, buffer, len(data)).wait(5)
        mock_service.process.send_signal(number)
        assert mock_service.process.wait(timeout=5) == 0
    assert "session stats" in mock_service.log()
