"""module: service linkage; responsibility: verify standalone runtime dependencies.

operations: pytest case; entities: none;
tests: the service loads without an embedded Python runtime.
"""

import subprocess
from pathlib import Path


def test_service_runtime_does_not_require_libpython(executable: Path) -> None:
    process = subprocess.run(
        ["readelf", "--dynamic", str(executable)], capture_output=True, text=True
    )
    assert process.returncode == 0, process.stderr
    assert "libpython" not in process.stdout
