"""module: bridge; responsibility: encode operations on an owned bridge session.

operations: Session exchange/register; entities: Session;
tests: kernel/test_batch, test_pool, test_copyout_faults, test_lifecycle.
"""

from collections.abc import Sequence
from types import TracebackType

from rkmpp_bridge_uapi import (
    EXCHANGE,
    REGISTER_POOL,
    VERSION,
    Batch,
    Message,
    Pool,
)

from tests.support.outcome import Ok, Result
from tests.support.syscalls import OwnedFd, ioctl


class Session:
    def __init__(self, control: OwnedFd) -> None:
        self._control: OwnedFd = control

    @property
    def fd(self) -> int:
        return self._control.fd

    def close(self) -> None:
        self._control.close()

    def __enter__(self) -> Session:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()

    def exchange(self, replies: Sequence[Message] = ()) -> Result[Batch]:
        assert len(replies) <= 16
        batch = Batch(version=VERSION, reply_count=len(replies))
        for index, reply in enumerate(replies):
            assert batch.set_reply(index, reply)
        status = ioctl(self.fd, EXCHANGE, batch)
        return Ok(batch) if isinstance(status, Ok) else status

    def register(self, kind: int, epoch: int, pool_id: int) -> Result[Pool]:
        pool = Pool(version=VERSION, kind=kind, epoch=epoch, pool_id=pool_id)
        result = ioctl(self.fd, REGISTER_POOL, pool)
        return Ok(pool) if isinstance(result, Ok) else result
