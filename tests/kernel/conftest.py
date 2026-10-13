"""module: kernel fixtures; responsibility: own both sides of a real kernel session.

operations: raw_session, packet_session; entities: none;
tests: test_batch, test_pool, test_frame_reply, test_layout_reply,
test_copyout_faults, test_lifecycle.
"""

import ctypes
from collections.abc import Generator
from pathlib import Path

import pytest

from tests.support.bridge_abi import Message
from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted
from tests.support.queue import Queue, QueueType
from tests.support.stream import Codec
from tests.support.video_formats import set_format


@pytest.fixture(scope="session", autouse=True)
def kernel_available(kernel_idle: Path) -> None:
    assert kernel_idle.is_char_device()


@pytest.fixture
def raw_session(kernel_owner: Path) -> Generator[Peer]:
    with accepted(Peer.open(kernel_owner, Path("/dev/video0"))) as peer:
        yield peer


@pytest.fixture
def packet_session(raw_session: Peer) -> tuple[Peer, tuple[Message, ...]]:
    video = raw_session.video
    _ = accepted(set_format(video.fd, QueueType.OUTPUT, Codec.H264.fourcc, size=4096))
    output = Queue(video.fd, QueueType.OUTPUT)
    assert accepted(output.request(3)) == 3
    accepted(output.stream_on())
    for index in range(3):
        accepted(output.queue(index, 16, index * 100000))
    batch = accepted(raw_session.service.exchange())
    assert batch.event_count == 3
    packets = tuple(
        Message.from_buffer_copy(batch.events, i * ctypes.sizeof(Message))
        for i in range(3)
    )
    return raw_session, packets
