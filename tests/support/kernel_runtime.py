"""module: kernel_runtime; responsibility: verify real-kernel test readiness.

operations: verify, exclusive; entities: none; tests: kernel_runtime_tests.
Readiness does not imply that the running module was built from this checkout.
"""

import errno
from pathlib import Path

from tests.support.outcome import Ok, Rejected, Result
from tests.support.syscalls import open_fd

_MODULES = Path("/sys/module")


def verify(modules: Path = _MODULES) -> Result[Path]:
    if not (modules / "v4l2_rkmpp_bridge").is_dir():
        return Rejected(
            errno.ENOENT,
            "Kernel module v4l2_rkmpp_bridge is not loaded. Manually install and load the module before running integration tests.",
        )
    return Ok(Path("/dev/v4l2-rkmpp-control"))


def exclusive(control: Path) -> Result[None]:
    opened = open_fd(control)
    if isinstance(opened, Rejected):
        if opened.code == errno.EBUSY:
            return Rejected(
                opened.code,
                "Kernel integration tests require the Python service substitute to exclusively hold the control device. Manually stop the running "
                + "v4l2-stateful-rkmpp service and rerun the tests; the tests do not stop it automatically.",
            )
        if opened.code in (errno.EACCES, errno.EPERM):
            return Rejected(
                opened.code,
                f"Kernel integration tests cannot read or write control device {control}. Apply the udev rules in "
                + "../armbian-binary-pkgbuilds/v4l2-stateful-rkmpp-git/70-v4l2-stateful-rkmpp.rules and ensure the current user "
                + "belongs to the video group and device permissions are root:video 0660. Root is not required to run kernel integration tests.",
            )
        return opened
    with opened.value:
        return Ok(None)
