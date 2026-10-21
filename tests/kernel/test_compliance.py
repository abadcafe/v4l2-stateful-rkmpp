"""module: test_compliance; responsibility: enforce public V4L2 conformance.

operations: pytest case; entities: none; tests: installed compliance command.
The real loaded driver is exercised exclusively through its public device.
"""

import subprocess
from pathlib import Path


def test_v4l2_compliance_accepts_stateful_decoder(kernel_idle: Path) -> None:
    assert kernel_idle.is_char_device()
    result = subprocess.run(
        ["v4l2-compliance", "-d", "/dev/video0"],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    # External failure on this machine's v4l2-utils 1.32.0-2 build:
    # its initial codec enumeration stops at AV01 without querying index 5.
    # The resulting missing stateful-decoder classification makes testEvents
    # reject the required MIN_BUFFERS_FOR_CAPTURE control. Independent ENUM_FMT
    # and QUERYCTRL tests verify both the AV01 format and the read-only control.
    # Upstream determine_codec_mask includes AV1; the installed binary's ioctl
    # trace reproduces the early return. Do not remove AV1 or its required
    # controls to accommodate this external detector, and do not skip the test.
    assert result.returncode == 0, result.stdout + result.stderr
