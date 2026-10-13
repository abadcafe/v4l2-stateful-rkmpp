"""module: service_runtime; responsibility: validate the running broker service.

operations: Running, locate, verify, sample; entities: Running;
tests: service_runtime_tests. Only query-only status and device exclusivity are used.
"""

import errno
from pathlib import Path

from pydantic import BaseModel

from tests.support import build_identity, diagnostics, syscalls
from tests.support.diagnostics import Control, Identity, Snapshot
from tests.support.outcome import Ok, Rejected, Result

ENDPOINT = Path("/run/v4l2-stateful-rkmpp/status.sock")
_CONTROL = Path("/dev/v4l2-rkmpp-control")


class Running(BaseModel, frozen=True):
    identity: Identity
    endpoint: Path


def _stop(details: str) -> str:
    return (
        details
        + " Manually stop the service (use systemctl stop for a systemd-managed service), then rerun the browser integration tests. No manual service start is required; the tests will start the latest build."
    )


def verify(
    binaries: tuple[Path, ...], endpoint: Path = ENDPOINT, control: Path = _CONTROL
) -> Result[Running | None]:
    if not binaries:
        return Rejected(
            errno.EINVAL,
            "No service binaries built from the current source were provided",
        )
    expected: set[str] = set()
    for binary in binaries:
        fingerprint = build_identity.service_id(binary)
        if isinstance(fingerprint, Rejected):
            return fingerprint
        expected.add(fingerprint.value)
    located = locate(endpoint, control)
    if isinstance(located, Rejected) or located.value is None:
        return located
    actual = located.value
    if actual.identity.build_id not in expected:
        return Rejected(
            errno.ESTALE,
            _stop(
                f"Running service PID {actual.identity.pid} does not match a binary built from the current source."
            ),
        )
    return located


def locate(
    endpoint: Path = ENDPOINT, control: Path = _CONTROL
) -> Result[Running | None]:
    status = diagnostics.query(endpoint)
    if isinstance(status, Rejected) and status.code not in (
        errno.ENOENT,
        errno.ECONNREFUSED,
    ):
        return Rejected(
            status.code,
            _stop(status.operation + ". Cannot verify the running service."),
        )
    opened = syscalls.open_fd(control)
    if isinstance(status, Rejected):
        if isinstance(opened, Ok):
            with opened.value:
                return Ok(None)
        if opened.code == errno.EBUSY:
            return Rejected(
                errno.EBUSY,
                _stop(
                    "Control device is busy, but no verifiable service diagnostics endpoint is available."
                ),
            )
        return opened
    if isinstance(opened, Ok):
        opened.value.close()
        return Rejected(
            errno.EPROTO,
            "Diagnostics endpoint exists, but the service does not exclusively hold the control device",
        )
    if opened.code != errno.EBUSY:
        return opened
    try:
        metadata = control.stat()
    except OSError as error:
        return Rejected(
            error.errno or 0, f"Read control device identity {control}: {error}"
        )
    actual = status.value
    if actual.identity.control != Control(
        device=metadata.st_dev, inode=metadata.st_ino, raw_device=metadata.st_rdev
    ):
        return Rejected(
            errno.ESTALE,
            _stop(
                "Control device reported by service diagnostics does not match the current device."
            ),
        )
    if actual.state != "ready":
        return Rejected(
            errno.ESHUTDOWN,
            "Service is stopping; wait for it to exit before rerunning the integration tests",
        )
    return Ok(Running(identity=actual.identity, endpoint=endpoint))


def sample(running: Running) -> Result[Snapshot]:
    status = diagnostics.query(running.endpoint)
    if isinstance(status, Rejected):
        return status
    if status.value.identity != running.identity:
        return Rejected(
            errno.ESTALE,
            "Service process identity changed; rerun the browser integration tests",
        )
    if status.value.state != "ready":
        return Rejected(errno.ESHUTDOWN, "Service is stopping")
    return status
