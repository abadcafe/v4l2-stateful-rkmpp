"""module: kernel_runtime; responsibility: verify real-kernel test readiness.

operations: verify, exclusive; entities: none; tests: kernel_runtime_tests.
The loaded ELF note is a public sysfs interface, independent of installed .ko files.
"""

import errno
from pathlib import Path

from tests.support.build_identity import module_note
from tests.support.outcome import Ok, Rejected, Result
from tests.support.syscalls import open_fd

_MODULES = Path("/sys/module")
_UPDATE = (
    "Manually stop the service and playback using the bridge device, unload the old module, then install and load "
    "kernel/v4l2_rkmpp_bridge.ko built with make -C kernel and rerun the integration tests. "
    "The tests do not automatically stop services or install or replace kernel modules."
)


def verify(root: Path, modules: Path = _MODULES) -> Result[Path]:
    module = modules / "v4l2_rkmpp_bridge"
    if not module.is_dir():
        return Rejected(
            errno.ENOENT, f"Kernel module v4l2_rkmpp_bridge is not loaded. {_UPDATE}"
        )
    expected = module_note(root)
    if isinstance(expected, Rejected):
        return expected
    try:
        loaded = (module / "notes/.note.gnu.build-id").read_bytes()
    except OSError as error:
        return Rejected(
            error.errno or 0,
            f"Cannot read the loaded kernel module ELF build ID: {error}. {_UPDATE}",
        )
    if loaded != expected.value:
        return Rejected(
            errno.ESTALE,
            f"Loaded kernel module does not match the current source build (build ID mismatch). {_UPDATE}",
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
                + "../armbian-binary-pkgbuilds/v4l2-stateful-rkmpp/70-v4l2-stateful-rkmpp.rules and ensure the current user "
                + "belongs to the video group and device permissions are root:video 0660. Root is not required to run kernel integration tests.",
            )
        return opened
    with opened.value:
        return Ok(None)
