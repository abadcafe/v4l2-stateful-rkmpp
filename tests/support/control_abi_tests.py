"""module: control_abi_tests; responsibility: specify V4L2 control wire layouts.

operations: pytest cases; entities: none; tests: full compiled field comparisons.
"""

import ctypes

import pytest

from tests.support.control_abi import Control, QueryControl, QueryMenu
from tests.support.outcome import accepted
from tests.support.uapi import verify


@pytest.mark.parametrize(
    ("wire_type", "c_type", "fields"),
    [
        (Control, "v4l2_control", (("id", "id"), ("value", "value"))),
        (
            QueryControl,
            "v4l2_queryctrl",
            (
                ("id", "id"),
                ("kind", "type"),
                ("name", "name"),
                ("minimum", "minimum"),
                ("maximum", "maximum"),
                ("step", "step"),
                ("default", "default_value"),
                ("flags", "flags"),
                ("reserved", "reserved"),
            ),
        ),
        (
            QueryMenu,
            "v4l2_querymenu",
            (
                ("id", "id"),
                ("index", "index"),
                ("name", "name"),
                ("reserved", "reserved"),
            ),
        ),
    ],
)
def test_control_all_fields_match_the_compiled_linux_header(
    wire_type: type[ctypes.Structure],
    c_type: str,
    fields: tuple[tuple[str, str], ...],
) -> None:
    assert accepted(verify(wire_type, c_type, fields)) is None
