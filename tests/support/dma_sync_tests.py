"""module: dma_sync_tests; responsibility: specify restartable DMA CPU sync.

operations: pytest cases; entities: none; tests: read/write start/end retries
and terminal errors through the public syscall dependency.
"""

import ctypes
import errno
import sys
from collections.abc import Callable

import pytest

from tests.support import syscalls
from tests.support.dma_sync import finish_read, finish_write, start_read, start_write
from tests.support.outcome import Ok, Rejected, Result, accepted


@pytest.mark.parametrize(
    ("operation", "flags"),
    [(start_read, 1), (finish_read, 5), (start_write, 3), (finish_write, 7)],
)
def test_sync_retries_transient_errors_with_unchanged_flags(
    monkeypatch: pytest.MonkeyPatch,
    operation: Callable[[int], Result[None]],
    flags: int,
) -> None:
    responses = iter((errno.EINTR, errno.EAGAIN, 0))
    calls: list[int] = []

    def interrupted(
        fd: int, request: int, payload: ctypes.Structure | int = 0
    ) -> Result[int]:
        assert fd == 42 and request == syscalls.command(1, "b", 0, 8)
        assert isinstance(payload, ctypes.Structure)
        calls.append(int.from_bytes(bytes(payload), sys.byteorder))
        code = next(responses)
        return Rejected(code, "DMA sync interrupted") if code else Ok(0)

    monkeypatch.setattr(syscalls, "ioctl", interrupted)
    assert accepted(operation(42)) is None
    assert calls == [flags] * 3


@pytest.mark.parametrize(
    "operation", [start_read, finish_read, start_write, finish_write]
)
def test_sync_returns_terminal_error_without_retry(
    monkeypatch: pytest.MonkeyPatch, operation: Callable[[int], Result[None]]
) -> None:
    calls = 0

    def failed(
        fd: int, request: int, payload: ctypes.Structure | int = 0
    ) -> Result[int]:
        nonlocal calls
        assert fd == 42 and request == syscalls.command(1, "b", 0, 8)
        assert isinstance(payload, ctypes.Structure)
        calls += 1
        return Rejected(errno.EIO, "DMA sync failed")

    monkeypatch.setattr(syscalls, "ioctl", failed)
    result = operation(42)
    assert isinstance(result, Rejected) and result.code == errno.EIO
    assert calls == 1
