"""module: service_runtime_discovery_tests; responsibility: define build-neutral discovery.

operations: pytest cases; entities: none; tests: locate actual service identity.
The tool records the running build; acceptance verify still requires current builds.
"""

import errno
import os
from pathlib import Path

import pytest

from tests.support import diagnostics, syscalls
from tests.support.diagnostics import Control, Identity, Snapshot
from tests.support.outcome import Ok, Rejected, Result, accepted
from tests.support.service_runtime import locate


def test_locate_reports_actual_build_without_compiling_or_replacing_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    control = tmp_path / "control"
    _ = control.write_bytes(b"")
    metadata = control.stat()
    snapshot = Snapshot(
        protocol=1,
        identity=Identity(
            pid=os.getpid(),
            uid=os.geteuid(),
            start_ticks=1,
            build_id="0123456789abcdef",
            control=Control(
                device=metadata.st_dev,
                inode=metadata.st_ino,
                raw_device=metadata.st_rdev,
            ),
        ),
        state="ready",
        cpu_seconds=0,
        fds=4,
        threads=2,
        sessions=(),
    )

    def query(_: Path) -> Result[Snapshot]:
        return Ok(snapshot)

    def busy(_: Path) -> Result[syscalls.OwnedFd]:
        return Rejected(errno.EBUSY, "occupied")

    monkeypatch.setattr(diagnostics, "query", query)
    monkeypatch.setattr(syscalls, "open_fd", busy)
    running = accepted(locate(tmp_path / "socket", control))
    assert running is not None and running.identity == snapshot.identity


def test_locate_free_control_without_status_allows_owned_start(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    control = tmp_path / "control"
    _ = control.write_bytes(b"")

    def missing(_: Path) -> Result[Snapshot]:
        return Rejected(errno.ENOENT, "missing")

    monkeypatch.setattr(diagnostics, "query", missing)
    assert accepted(locate(tmp_path / "socket", control)) is None
