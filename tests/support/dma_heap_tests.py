"""module: dma_heap_tests; responsibility: specify mock buffer ownership.

operations: pytest cases; entities: none; tests: allocation/write/read/bounds.
"""

import os

import pytest

from tests.support.dma_heap import Buffer
from tests.support.dmabuf import read_bytes
from tests.support.outcome import Rejected, accepted


def test_allocate_write_and_read_use_real_synchronized_dma_buf() -> None:
    buffer = accepted(Buffer.allocate(4096))
    with buffer:
        fd = buffer.fd
        assert buffer.size == 4096 and not os.get_inheritable(fd)
        assert os.fstat(fd).st_size >= buffer.size
        assert accepted(buffer.write(b"payload", 19)) is None
        assert accepted(read_bytes(fd, 7, 19)) == b"payload"
    assert not os.path.exists(f"/proc/self/fd/{fd}")
    buffer.close()


@pytest.mark.parametrize("size", [0, -1, 128 * 1024 * 1024 + 1])
def test_allocate_invalid_extent_is_rejected(size: int) -> None:
    assert isinstance(Buffer.allocate(size), Rejected)


@pytest.mark.parametrize("offset", [-1, 4096])
def test_write_invalid_region_preserves_buffer(offset: int) -> None:
    with accepted(Buffer.allocate(4096)) as buffer:
        _ = accepted(buffer.write(b"known"))
        assert isinstance(buffer.write(b"x", offset), Rejected)
        assert accepted(read_bytes(buffer.fd, 5)) == b"known"
