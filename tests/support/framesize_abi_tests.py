"""module: framesize_abi_tests; responsibility: specify V4L2 extent wire layouts.

operations: pytest cases; entities: none; tests: compiled frame-size structures.
"""

import ctypes

import pytest

from tests.support.framesize_abi import FrameSize, Stepwise
from tests.support.outcome import accepted
from tests.support.uapi import verify


@pytest.mark.parametrize(
    ("wire_type", "c_type", "fields"),
    [
        (
            FrameSize,
            "v4l2_frmsizeenum",
            (
                ("index", "index"),
                ("fourcc", "pixel_format"),
                ("kind", "type"),
                ("stepwise", "stepwise"),
                ("reserved", "reserved"),
            ),
        ),
        (
            Stepwise,
            "v4l2_frmsize_stepwise",
            tuple(
                (field, field)
                for field in (
                    "min_width",
                    "max_width",
                    "step_width",
                    "min_height",
                    "max_height",
                    "step_height",
                )
            ),
        ),
    ],
)
def test_framesize_all_fields_match_the_compiled_linux_header(
    wire_type: type[ctypes.Structure],
    c_type: str,
    fields: tuple[tuple[str, str], ...],
) -> None:
    assert accepted(verify(wire_type, c_type, fields)) is None
