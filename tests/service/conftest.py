"""module: service fixtures; responsibility: own kernel-isolated daemon lifetimes.

operations: mock_factory/mock_service; entities: MockFactory;
tests: service process acceptance; the real bridge node is never opened.
"""

from collections.abc import Callable, Generator
from pathlib import Path

import pytest

from tests.support.mock_process import Process
from tests.support.outcome import accepted

type MockFactory = Callable[[], Process]


@pytest.fixture
def mock_factory(executable: Path, tmp_path: Path) -> Generator[MockFactory]:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    processes: list[Process] = []

    def start() -> Process:
        process = accepted(
            Process.open(executable, library, tmp_path / f"mock-{len(processes)}")
        )
        processes.append(process)
        return process

    try:
        yield start
    finally:
        for process in reversed(processes):
            process.close()


@pytest.fixture
def mock_service(mock_factory: MockFactory) -> Process:
    return mock_factory()
