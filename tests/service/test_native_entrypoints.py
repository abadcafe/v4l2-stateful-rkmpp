"""module: test_native_entrypoints; responsibility: verify preload syscall isolation.

operations: process acceptance cases; entities: none;
tests: invalid addresses, protected-page boundary, missing mock destination.
Independent Python processes call only the public open64 interface through ctypes.
"""

import ctypes
import errno
import mmap
import os
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest
from pydantic import TypeAdapter

_libc = ctypes.CDLL(None, use_errno=True)
_libc.open64.argtypes = [ctypes.c_void_p, ctypes.c_int]
_libc.open64.restype = ctypes.c_int
_libc.mprotect.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int]
_libc.mprotect.restype = ctypes.c_int


def _open_status(address: int) -> tuple[int, int]:
    fd = TypeAdapter(int).validate_python(
        _libc.open64(ctypes.c_void_p(address), os.O_RDONLY), strict=True
    )
    error = ctypes.get_errno()
    if fd >= 0:
        os.close(fd)
    return fd, error


def _invalid_addresses() -> None:
    maximum = (1 << (ctypes.sizeof(ctypes.c_void_p) * 8)) - 1
    for address in (0, 1, maximum):
        assert _open_status(address) == (-1, errno.EFAULT)


def _boundary_path() -> None:
    page = mmap.PAGESIZE
    with mmap.mmap(
        -1,
        page * 2,
        flags=mmap.MAP_PRIVATE | mmap.MAP_ANONYMOUS,
        prot=mmap.PROT_READ | mmap.PROT_WRITE,
    ) as mapping:
        view = ctypes.c_char.from_buffer(mapping)
        address = ctypes.addressof(view)
        del view
        name = b"/dev/null\0"
        mapping[page - len(name) : page] = name
        protected = TypeAdapter(int).validate_python(
            _libc.mprotect(ctypes.c_void_p(address + page), page, 0), strict=True
        )
        assert protected == 0, os.strerror(ctypes.get_errno())
        fd, error = _open_status(address + page - len(name))
        assert fd >= 0, os.strerror(error)


def _missing_destination() -> None:
    name = ctypes.create_string_buffer(b"/dev/v4l2-rkmpp-control")
    assert _open_status(ctypes.addressof(name)) == (-1, errno.EINVAL)


@pytest.mark.parametrize(
    "probe",
    [_invalid_addresses, _boundary_path, _missing_destination],
    ids=["invalid", "boundary", "destination"],
)
def test_open64_preserves_native_pointer_validation_and_mock_isolation(
    executable: Path, probe: Callable[[], None]
) -> None:
    environment = dict(os.environ)
    environment["LD_PRELOAD"] = str(executable.parent / "libv4l2_rkmpp_kernel_mock.so")
    environment["RKMPP_TEST_KERNEL_SOCKET"] = "unused"
    _ = environment.pop("RKMPP_TEST_KERNEL_CONTROL", None)
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            f"from {__name__} import {probe.__name__}; {probe.__name__}()",
        ],
        env=environment,
        cwd=Path(__file__).resolve().parents[2],
        capture_output=True,
        text=True,
        timeout=5,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
