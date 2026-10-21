"""module: tests.kernel.test_copyout_faults; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_exchange_copyout_fault_preserves_unconsumed_packet_order
    test_register_pool_copyout_fault_rolls_back_every_installed_fd
    test_register_pool_failed_copyout_never_publishes_usable_descriptors
"""

import errno
import os
import threading

from rkmpp_bridge_uapi import (
    EXCHANGE,
    REGISTER_POOL,
    VERSION,
    Batch,
    Event,
    Message,
    Pool,
)

from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted, present
from tests.support.queue import Queue, QueueType
from tests.support.stream import Codec
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
    result = Batch(version=VERSION).ioctl_readonly(raw_session.service.fd, EXCHANGE)
    assert result == -errno.EFAULT
    batch = accepted(raw_session.service.exchange())
    assert batch.event_count == 3
    packets = [present(batch.event(i)) for i in range(3)]
    assert [(p.kind, p.index) for p in packets] == [(Event.PACKET, i) for i in range(3)]


def test_register_pool_copyout_fault_rolls_back_every_installed_fd(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    session, packets = packet_session
    packet = packets[0]
    payload = Pool(version=VERSION, kind=10, epoch=packet.epoch, pool_id=packet.pool_id)
    before = len(os.listdir("/proc/self/fd"))
    result = payload.ioctl_readonly(session.service.fd, REGISTER_POOL)
    assert result == -errno.EFAULT
    assert len(os.listdir("/proc/self/fd")) == before
    pool = accepted(session.service.register(10, packet.epoch, packet.pool_id))
    try:
        assert pool.count == 3
    finally:
        for index in range(pool.count):
            descriptor = present(pool.descriptor(index))
            os.close(descriptor.fd)
    assert len(os.listdir("/proc/self/fd")) == before


def test_register_pool_failed_copyout_never_publishes_usable_descriptors(
    raw_session: Peer,
) -> None:
    queue = Queue(raw_session.video.fd, QueueType.OUTPUT)
    assert accepted(queue.request(32)) == 32
    accepted(queue.stream_on())
    accepted(queue.queue(0, 16))
    packet = present(accepted(raw_session.service.exchange()).event(0))
    identities: set[tuple[int, int]] = set()
    for index in range(32):
        with accepted(queue.export(index)) as owner:
            metadata = os.fstat(owner.fd)
            identities.add((metadata.st_dev, metadata.st_ino))
    finished = threading.Event()
    started = threading.Event()
    exposed: list[int] = []

    def observe() -> None:
        started.set()
        while not finished.is_set():
            for entry in os.listdir("/proc/self/fd"):
                try:
                    metadata = os.fstat(int(entry))
                except OSError as error:
                    assert error.errno == errno.EBADF
                    continue
                if (metadata.st_dev, metadata.st_ino) in identities:
                    exposed.append(int(entry))

    observer = threading.Thread(target=observe)
    observer.start()
    payload = Pool(version=VERSION, kind=10, epoch=packet.epoch, pool_id=packet.pool_id)
    try:
        assert started.wait(timeout=5)
        for _ in range(256):
            result = payload.ioctl_readonly(raw_session.service.fd, REGISTER_POOL)
            assert result == -errno.EFAULT
    finally:
        finished.set()
        observer.join(timeout=5)
    assert not observer.is_alive()
    # Failed copyout cannot transfer ownership, even transiently. Otherwise
    # a concurrent close/reuse can make rollback close an unrelated file.
    assert not exposed, f"failed registration exposed live descriptors: {exposed}"
