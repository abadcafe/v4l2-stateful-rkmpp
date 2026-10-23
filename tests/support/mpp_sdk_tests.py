"""module: mpp_sdk_tests; responsibility: specify compile-time MPP ABI admission.

operations: pytest cases; entities: none; tests: changed SDK declarations must
be rejected before a service can call an incompatible foreign operation.
"""

import os
import subprocess
from pathlib import Path


def test_build_rejects_sdk_without_required_frame_operation(tmp_path: Path) -> None:
    root = Path(__file__).resolve().parents[2]
    include = tmp_path / "sdk headers"
    (include / "rockchip").mkdir(parents=True)
    # This fixture changes only the advertised SDK contract, never the installed
    # library. A handwritten extern declaration would silently bypass it.
    _ = (include / "rockchip/mpp_frame.h").write_text(
        "\n".join(
            [
                "#define mpp_frame_get_width sdk_unavailable_frame_width",
                "#include_next <rockchip/mpp_frame.h>",
                "",
            ]
        )
    )
    environment = dict(os.environ)
    environment["BINDGEN_EXTRA_CLANG_ARGS"] = "-I'" + str(include) + "'"
    checked = subprocess.run(
        ["cargo", "check", "--locked", "--offline", "-p", "rkmpp"],
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    assert checked.returncode != 0, "handwritten FFI bypassed the selected MPP SDK"
    assert "mpp_frame_get_width" in checked.stderr
