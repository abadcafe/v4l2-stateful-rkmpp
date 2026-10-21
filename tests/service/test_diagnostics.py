"""module: test_diagnostics; responsibility: accept service diagnostic lifecycle.

operations: pytest cases; entities: none; tests: identity, resource accuracy,
running inode, socket cleanup, request isolation. Uses only process interfaces.
"""

import errno
import os
import shutil
import socket
from contextlib import ExitStack
from pathlib import Path

from tests.support.build_identity import service_id
from tests.support.diagnostics import measure, query
from tests.support.mock_process import Process
from tests.support.outcome import Rejected, accepted
from tests.support.syscalls import OwnedFd


def test_diagnostics_reports_actual_image_credentials_and_exact_resources(
    mock_service: Process, executable: Path
) -> None:
    first = accepted(query(mock_service.diagnostics))
    assert first.identity.pid == mock_service.process.pid
    assert first.identity.uid == os.geteuid()
    assert first.identity.build_id == accepted(service_id(executable))
    proc = Path(f"/proc/{mock_service.process.pid}")
    for _ in range(10):
        status = accepted(query(mock_service.diagnostics))
        assert status.identity == first.identity and status.state == "ready"
        assert status.fds == len(tuple((proc / "fd").iterdir()))
        assert status.threads == len(tuple((proc / "task").iterdir()))
        assert status.cpu_seconds >= first.cpu_seconds and not status.sessions
    assert mock_service.diagnostics.stat().st_mode & 0o777 == 0o660


def test_diagnostics_replaced_disk_binary_preserves_running_inode_identity(
    executable: Path, tmp_path: Path
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    image = tmp_path / "service"
    _ = shutil.copyfile(executable, image)
    image.chmod(0o700)
    identity = accepted(service_id(image))
    with accepted(Process.open(image, library, tmp_path / "mock")) as process:
        replacement = tmp_path / "new-image"
        _ = replacement.write_bytes(b"replacement on disk")
        _ = replacement.replace(image)
        assert (
            "(deleted)"
            in Path(f"/proc/{process.process.pid}/exe").readlink().as_posix()
        )
        assert accepted(query(process.diagnostics)).identity.build_id == identity
        assert (
            image.read_bytes() == b"replacement on disk"
            and process.process.poll() is None
        )


def test_diagnostics_exit_removes_socket_and_leaves_no_status(
    mock_service: Process,
) -> None:
    endpoint = mock_service.diagnostics
    assert accepted(query(endpoint)).state == "ready"
    mock_service.close()
    assert not endpoint.exists()
    result = query(endpoint)
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_diagnostics_mutation_request_cannot_stop_or_control_service(
    mock_service: Process,
) -> None:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(1)
        client.connect(str(mock_service.diagnostics))
        client.sendall(b"stop\n")
        assert b'"error"' in client.recv(4096)
    assert mock_service.process.poll() is None
    assert accepted(query(mock_service.diagnostics)).state == "ready"


def test_diagnostics_counts_inherited_resources_independently_of_parent_activity(
    executable: Path,
    tmp_path: Path,
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with (
        OwnedFd(os.open("/proc/self/fd", os.O_RDONLY | os.O_DIRECTORY)) as directory,
        ExitStack() as resources,
    ):
        # Hold a socket peer so process exit is observed without inspecting a reused fd number.
        channel, peer = socket.socketpair()
        _ = resources.enter_context(peer)
        owner = resources.enter_context(OwnedFd(channel.detach()))
        peer.settimeout(1)
        with accepted(
            Process.open(
                executable, library, tmp_path / "mock", inherited=(directory, owner)
            )
        ) as process:
            proc = Path(f"/proc/{process.process.pid}")
            assert (proc / "fd" / str(directory.fd)).is_dir()
            owner.close()
            baseline = accepted(query(process.diagnostics))
            for _ in range(10):
                with ExitStack() as unrelated:
                    for _ in range(32):
                        _ = unrelated.enter_context(Path("/dev/null").open("rb"))
                    status = accepted(query(process.diagnostics))
                    assert (status.fds, status.threads) == (
                        baseline.fds,
                        baseline.threads,
                    )
                    assert status.fds == len(tuple((proc / "fd").iterdir()))
                    assert status.threads == len(tuple((proc / "task").iterdir()))
        assert process.process.returncode == 0
        assert peer.recv(1) == b""
        assert not proc.exists() and not process.diagnostics.exists()


def test_diagnostics_measurement_matches_identity_without_resource_inventory(
    mock_service: Process,
) -> None:
    snapshot = accepted(query(mock_service.diagnostics))
    first = accepted(measure(mock_service.diagnostics))
    second = accepted(measure(mock_service.diagnostics))
    assert first.identity == snapshot.identity == second.identity
    assert first.state == second.state == "ready"
    assert second.cpu_seconds >= first.cpu_seconds >= snapshot.cpu_seconds
    assert set(first.model_dump()) == {"protocol", "identity", "state", "cpu_seconds"}
