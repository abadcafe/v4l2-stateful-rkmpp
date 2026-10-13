"""module: framesize_abi; responsibility: encode public V4L2 frame-size constraints.

operations: FrameSize, Stepwise; entities: two wire structures;
tests: framesize_abi_tests, kernel/test_formats.
"""

import ctypes
from typing import final


@final
class Stepwise(ctypes.Structure):
    min_width: int = 0
    max_width: int = 0
    step_width: int = 0
    min_height: int = 0
    max_height: int = 0
    step_height: int = 0
    _fields_ = [
        (name, ctypes.c_uint32)
        for name in (
            "min_width",
            "max_width",
            "step_width",
            "min_height",
            "max_height",
            "step_height",
        )
    ]


@final
class FrameSize(ctypes.Structure):
    index: int = 0
    fourcc: int = 0
    kind: int = 0
    stepwise: Stepwise = Stepwise()
    _fields_ = [
        ("index", ctypes.c_uint32),
        ("fourcc", ctypes.c_uint32),
        ("kind", ctypes.c_uint32),
        ("stepwise", Stepwise),
        ("reserved", ctypes.c_uint32 * 2),
    ]
