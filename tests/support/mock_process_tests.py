"""module: mock_process_tests; responsibility: specify isolated daemon interception.

operations: pytest cases; entities: none; tests: startup/session/error/termination.
"""

import errno
import os
import select
import signal
import subprocess
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest
from rkmpp_bridge_uapi import EXCHANGE, VERSION, Batch, Event

from tests.support import mock_process
from tests.support.mock_process import Peer, Process
from tests.support.mock_wire import Reply, Request
from tests.support.outcome import Rejected, Result, accepted, present
from tests.support.seqpacket import Socket
from tests.support.syscalls import OwnedFd


def test_close_joins_sessions_still_being_accepted(
    executable: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    constructing, publish = threading.Event(), threading.Event()
    handling, release = threading.Event(), threading.Event()
    listener_closed = threading.Event()

    class DelayedPeer(Peer):
        def __init__(self, endpoint: Socket, handler: mock_process.Handler) -> None:
            super().__init__(endpoint, handler)
            constructing.set()
            assert publish.wait(5)

    def handle(request: Request) -> Reply:
        assert request.command == EXCHANGE
        handling.set()
        assert release.wait(5)
        return Reply(code=errno.EPIPE)

    monkeypatch.setattr(mock_process, "Peer", DelayedPeer)
    library = executable.parent / "libv4l2_rkmpp_kernel_mock.so"
    process = accepted(Process.open(executable, library, tmp_path / "mock"))
    close_socket = Socket.close

    def terminate_observed() -> None:
        # End the owned daemon immediately so its own graceful shutdown
        # cannot accidentally supply the missing handler join.
        process.process.kill()
        _ = process.process.wait(timeout=5)

    def close_observed(endpoint: Socket) -> Result[None]:
        result = close_socket(endpoint)
        listener_closed.set()
        return result

    monkeypatch.setattr(Socket, "close", close_observed)
    monkeypatch.setattr(process.process, "terminate", terminate_observed)
    with ThreadPoolExecutor(max_workers=2) as executor:
        opening = executor.submit(process.session, handle)
        closing = None
        try:
            assert constructing.wait(5) and handling.wait(5)
            closing = executor.submit(process.close)
            assert listener_closed.wait(5)
            publish.set()
            _ = accepted(opening.result(timeout=5))
            # Accepted handlers remain owned until they finish, even if
            # acceptance was preempted before publishing the peer.
            _ = threading.Event().wait(0.1)
            assert not closing.done()
        finally:
            publish.set()
            release.set()
            if closing is not None:
                closing.result(timeout=5)
            else:
                process.close()


def test_close_forced_exit_observes_terminal_status_before_releasing_resources(
    executable: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    directory = tmp_path / "mock"
    process = accepted(Process.open(executable, library, directory))
    wait = process.process.wait
    observed: list[bool] = []
    with OwnedFd(os.open(directory / "ctl", os.O_RDONLY | os.O_NONBLOCK)) as reader:
        readiness = select.poll()
        readiness.register(reader.fd, select.POLLIN | select.POLLHUP)

        def delayed_wait(timeout: float | None = None) -> int:
            if timeout is not None:
                raise subprocess.TimeoutExpired(process.process.args, timeout)
            status = wait()
            # After the actual child exits, only Process's parent-side FIFO
            # writer prevents EOF. It must remain owned through this wait.
            observed.append(not readiness.poll(0))
            return status

        def delayed_termination() -> None:
            # Model a graceful signal whose delivery has not completed before
            # the first observation deadline; SIGKILL remains real.
            assert process.process.poll() is None

        try:
            with monkeypatch.context() as operations:
                operations.setattr(process.process, "terminate", delayed_termination)
                operations.setattr(process.process, "wait", delayed_wait)
                process.close()
            assert observed == [True]
            assert process.process.returncode == -signal.SIGKILL
            assert readiness.poll(1000) == [(reader.fd, select.POLLHUP)]
        finally:
            if process.process.poll() is None:
                process.process.kill()
            _ = wait()
            process.close()


@pytest.mark.parametrize(
    "timeout", [0.0, -1.0, float("nan"), float("inf"), -float("inf")]
)
def test_wait_invalid_deadline_rejects_without_waiting(timeout: float) -> None:
    endpoint, remote = accepted(Socket.pair())

    def handle(request: Request) -> Reply:
        return Reply(body=request.body)

    peer = Peer(endpoint, handle)
    try:
        result = peer.wait(timeout)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
    finally:
        peer.close()
        accepted(remote.close())


def test_process_session_close_uses_real_daemon_and_only_mock_endpoint(
    executable: Path, tmp_path: Path
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"

    def handle(request: Request) -> Reply:
        assert request.command == EXCHANGE
        batch = present(Batch.decode(request.body))
        assert batch.version == VERSION and batch.reply_count == 0
        batch.event_count = 1
        event = present(batch.event(0))
        event.version = VERSION
        event.kind = Event.CLOSE
        event.epoch = 1
        assert batch.set_event(0, event)
        return Reply(body=bytes(batch))

    process = accepted(Process.open(executable, library, tmp_path / "mock"))
    with process:
        peer = accepted(process.session(handle))
        assert accepted(peer.wait(5)) is None
        assert len(peer.calls()) == 1
        assert len(peer.calls()[0].body) == Batch.SIZE
        assert process.process.poll() is None
    assert process.process.returncode == 0
    assert "events=1" in process.log()


def test_process_rejected_accept_exits_with_original_errno(
    executable: Path, tmp_path: Path
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        assert accepted(process.reject_accept(errno.EIO)) is None
        assert process.process.wait(timeout=5) == 1
        assert "accept control" in process.log()


def test_open_missing_library_returns_rejected_without_spawning(
    executable: Path, tmp_path: Path
) -> None:
    result = Process.open(executable, tmp_path / "missing.so", tmp_path / "mock")
    assert isinstance(result, Rejected) and result.code == errno.ENOENT
    assert not (tmp_path / "mock").exists()


def test_process_environment_and_descriptors_never_select_real_bridge(
    executable: Path,
    tmp_path: Path,
) -> None:
    parent = dict(os.environ)
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        links = [
            path.readlink().as_posix()
            for path in Path(f"/proc/{process.process.pid}/fd").iterdir()
        ]
        assert str(tmp_path / "mock/ctl") in links
        assert "/dev/v4l2-rkmpp-control" not in links and "/dev/video0" not in links
        assert dict(os.environ) == parent
        assert not any(path == "/dev/v4l2-rkmpp-control" for path in links)


@pytest.mark.parametrize("code", [0, -1, 1 << 31])
def test_reject_accept_invalid_errno_preserves_idle_daemon(
    executable: Path, tmp_path: Path, code: int
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        result = process.reject_accept(code)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
        assert process.process.poll() is None


def test_session_invalid_response_closes_peer_and_reports_rejected(
    executable: Path,
    tmp_path: Path,
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:

        def handle(request: Request) -> Reply:
            assert request.command == EXCHANGE
            return Reply(code=-1)

        peer = accepted(process.session(handle))
        invalid = peer.wait(0)
        assert isinstance(invalid, Rejected) and invalid.code == errno.EINVAL
        result = peer.wait(5)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
        peer.close()
        result = peer.wake()
        assert isinstance(result, Rejected)
        assert process.process.poll() is None


def test_open_inherits_only_selected_descriptors_and_preserves_parent_owners(
    executable: Path,
    tmp_path: Path,
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    selected, private = tmp_path / "selected", tmp_path / "private"
    _ = selected.write_bytes(b"selected")
    _ = private.write_bytes(b"private")
    with (
        OwnedFd(os.open(selected, os.O_RDONLY)) as inherited,
        OwnedFd(os.open(private, os.O_RDONLY)) as excluded,
    ):
        with accepted(
            Process.open(executable, library, tmp_path / "mock", inherited=(inherited,))
        ) as process:
            links = tuple(
                path.readlink()
                for path in Path(f"/proc/{process.process.pid}/fd").iterdir()
            )
            assert selected in links and private not in links
        assert os.pread(inherited.fd, 8, 0) == b"selected"
        assert os.pread(excluded.fd, 7, 0) == b"private"
