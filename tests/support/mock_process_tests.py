"""module: mock_process_tests; responsibility: specify isolated daemon interception.

operations: pytest cases; entities: none; tests: startup/session/error/termination.
"""

import ctypes
import errno
import os
from pathlib import Path

import pytest

from tests.support.bridge_abi import EXCHANGE, VERSION, Batch, Event, Message
from tests.support.mock_process import Process
from tests.support.mock_wire import Reply, Request
from tests.support.outcome import Rejected, accepted


def test_process_session_close_uses_real_daemon_and_only_mock_endpoint(
    executable: Path, tmp_path: Path
) -> None:
    library = (
        executable.parent.parent / "kernel-mock/debug/libv4l2_rkmpp_kernel_mock.so"
    )

    def handle(request: Request) -> Reply:
        assert request.command == EXCHANGE
        batch = Batch.from_buffer_copy(request.body)
        assert batch.version == VERSION and batch.reply_count == 0
        batch.event_count = 1
        event = Message.from_buffer(batch.events)
        event.version = VERSION
        event.kind = Event.CLOSE
        event.epoch = 1
        return Reply(body=bytes(batch))

    process = accepted(Process.open(executable, library, tmp_path / "mock"))
    with process:
        peer = accepted(process.session(handle))
        assert accepted(peer.wait(5)) is None
        assert len(peer.calls()) == 1
        assert len(peer.calls()[0].body) == ctypes.sizeof(Batch)
        assert process.process.poll() is None
    assert process.process.returncode == 0
    assert "events=1" in process.log()


def test_process_rejected_accept_exits_with_original_errno(
    executable: Path, tmp_path: Path
) -> None:
    library = (
        executable.parent.parent / "kernel-mock/debug/libv4l2_rkmpp_kernel_mock.so"
    )
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
    library = (
        executable.parent.parent / "kernel-mock/debug/libv4l2_rkmpp_kernel_mock.so"
    )
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
    library = (
        executable.parent.parent / "kernel-mock/debug/libv4l2_rkmpp_kernel_mock.so"
    )
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        result = process.reject_accept(code)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
        assert process.process.poll() is None


def test_session_invalid_response_closes_peer_and_reports_rejected(
    executable: Path,
    tmp_path: Path,
) -> None:
    library = (
        executable.parent.parent / "kernel-mock/debug/libv4l2_rkmpp_kernel_mock.so"
    )
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
    from tests.support.syscalls import OwnedFd

    library = (
        executable.parent.parent / "kernel-mock/debug/libv4l2_rkmpp_kernel_mock.so"
    )
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
