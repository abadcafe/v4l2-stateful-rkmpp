"""module: tests.kernel.test_copyout_faults; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_exchange_copyout_fault_preserves_unconsumed_packet_order
    test_register_pool_copyout_fault_rolls_back_every_installed_fd


"""

import ctypes
import errno
import os

from tests.support.bridge_abi import (
    EXCHANGE,
    REGISTER_POOL,
    VERSION,
    Batch,
    Descriptor,
    Event,
    Message,
    Pool,
)
from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType
from tests.support.stream import Codec
from tests.support.syscalls import ioctl, readonly_payload
from tests.support.video_formats import set_format


def test_exchange_copyout_fault_preserves_unconsumed_packet_order(
    raw_session: Peer,
) -> None:
    _ = accepted(
        set_format(raw_session.video.fd, QueueType.OUTPUT, Codec.H264.fourcc, size=4096)
    )
    queue = Queue(raw_session.video.fd, QueueType.OUTPUT)
    assert accepted(queue.request(3)) == 3
    accepted(queue.stream_on())
    for index in range(3):
        accepted(queue.queue(index, 16))
    with readonly_payload(Batch(version=VERSION)) as address:
        result = ioctl(raw_session.service.fd, EXCHANGE, address)
        assert isinstance(result, Rejected) and result.code == errno.EFAULT
    batch = accepted(raw_session.service.exchange())
    assert batch.event_count == 3
    packets = [
        Message.from_buffer_copy(batch.events, i * ctypes.sizeof(Message))
        for i in range(3)
    ]
    assert [(p.kind, p.index) for p in packets] == [(Event.PACKET, i) for i in range(3)]


def test_register_pool_copyout_fault_rolls_back_every_installed_fd(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    session, packets = packet_session
    packet = packets[0]
    payload = Pool(version=VERSION, kind=10, epoch=packet.epoch, pool_id=packet.pool_id)
    before = len(os.listdir("/proc/self/fd"))
    with readonly_payload(payload) as address:
        result = ioctl(session.service.fd, REGISTER_POOL, address)
        assert isinstance(result, Rejected) and result.code == errno.EFAULT
    assert len(os.listdir("/proc/self/fd")) == before
    pool = accepted(session.service.register(10, packet.epoch, packet.pool_id))
    try:
        assert pool.count == 3
    finally:
        for index in range(pool.count):
            descriptor = Descriptor.from_buffer_copy(
                pool.buffers, index * ctypes.sizeof(Descriptor)
            )
            os.close(descriptor.fd)
    assert len(os.listdir("/proc/self/fd")) == before
