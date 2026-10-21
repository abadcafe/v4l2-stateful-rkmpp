"""module: tests.kernel.test_pool; responsibility: define the tested public contract.

operations: pytest cases; entities: InvalidField; tests:
    test_register_pool_before_buffer_allocation_is_stale
    test_register_pool_exports_cloexec_sized_buffers
    test_register_pool_rejects_invalid_request


"""

import errno
import os
from typing import Literal

import pytest
from rkmpp_bridge_uapi import REGISTER_POOL, VERSION, Message, Pool

from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted, present
from tests.support.syscalls import OwnedFd, ioctl

type InvalidField = Literal["version", "reserved", "kind", "epoch", "pool_id"]


def test_register_pool_before_buffer_allocation_is_stale(
    raw_session: Peer,
) -> None:
    result = raw_session.service.register(10, 1, 0)
    assert isinstance(result, Rejected) and result.code == errno.ESTALE


def test_register_pool_exports_cloexec_sized_buffers(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    session, packets = packet_session
    packet = packets[0]
    pool = accepted(session.service.register(10, packet.epoch, packet.pool_id))
    assert pool.count == 3
    fds: list[OwnedFd] = []
    try:
        for index in range(pool.count):
            descriptor = present(pool.descriptor(index))
            owner = OwnedFd(descriptor.fd)
            fds.append(owner)
            assert not os.get_inheritable(owner.fd)
            assert (
                descriptor.size >= 16 and os.fstat(owner.fd).st_size >= descriptor.size
            )
        assert len({owner.fd for owner in fds}) == 3
    finally:
        for owner in fds:
            owner.close()


@pytest.mark.parametrize(
    ("field", "expected"),
    [
        pytest.param("version", errno.EINVAL, id="version"),
        pytest.param("reserved", errno.EINVAL, id="reserved"),
        pytest.param("kind", errno.EINVAL, id="queue-type"),
        pytest.param("epoch", errno.ESTALE, id="epoch"),
        pytest.param("pool_id", errno.ESTALE, id="generation"),
    ],
)
def test_register_pool_rejects_invalid_request(
    packet_session: tuple[Peer, tuple[Message, ...]],
    field: InvalidField,
    expected: int,
) -> None:
    session, packets = packet_session
    packet = packets[0]
    pool = Pool(version=VERSION, kind=10, epoch=packet.epoch, pool_id=packet.pool_id)
    if field == "reserved":
        pool.reserved = [1, 0]
    elif field == "kind":
        pool.kind = 0
    elif field == "version":
        pool.version += 1
    elif field == "epoch":
        pool.epoch += 1
    else:
        pool.pool_id += 1
    result = ioctl(session.service.fd, REGISTER_POOL, pool)
    assert isinstance(result, Rejected) and result.code == expected
