"""module: uapi_tests; responsibility: specify compiled public-header comparisons.

operations: pytest cases; entities: one synthetic public payload;
tests: valid layout, swapped same-size fields, bad field/constant/compiler input.
"""

import ctypes
import errno
from typing import final

from tests.support.outcome import Rejected, accepted
from tests.support.uapi import verify


@final
class Swapped(ctypes.Structure):
    _fields_ = [("size", ctypes.c_uint32), ("fd", ctypes.c_int32)]


def test_verify_compiles_exact_struct_size_alignment_and_all_fields() -> None:
    assert (
        accepted(
            verify(Swapped, "rb_registered_buffer", (("size", "fd"), ("fd", "size")))
        )
        is None
    )


def test_verify_same_size_swapped_fields_detects_offset_mismatch() -> None:
    result = verify(Swapped, "rb_registered_buffer", (("size", "size"), ("fd", "fd")))
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
    assert "offset" in result.operation


def test_verify_missing_public_field_returns_rejected() -> None:
    result = verify(Swapped, "rb_registered_buffer", (("missing", "fd"),))
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_verify_wrong_constant_detects_contract_mismatch() -> None:
    result = verify(Swapped, "rb_registered_buffer", (), (("RB_VERSION", 99),))
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_verify_unsafe_compiler_identifiers_return_rejected() -> None:
    result = verify(Swapped, "rb_registered_buffer;", ())
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
