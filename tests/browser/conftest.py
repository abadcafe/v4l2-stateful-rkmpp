"""module: browser fixtures; responsibility: lease functional acceptance sessions.

operations: browser_factory, bridge_service, browser_binaries;
entities: BrowserFactory; tests: test_graphics, test_playback, test_sessions.
Frequency policy and utilization sampling belong exclusively to tests/tools.
"""

from collections.abc import Callable, Generator
from pathlib import Path

import pytest

from tests.support.browser import Browser
from tests.support.build_identity import release_binary
from tests.support.outcome import accepted
from tests.support.service_process import Lease
from tests.support.service_runtime import ENDPOINT, Running, sample, verify

type BrowserFactory = Callable[[tuple[str, ...]], Browser]


@pytest.fixture
def browser_factory(
    bridge_service: Running, artifacts: Path, tmp_path: Path
) -> Generator[BrowserFactory]:
    _ = accepted(sample(bridge_service))
    media = tuple(
        file for file in artifacts.iterdir() if file.suffix in (".webm", ".mp4")
    )
    browsers: list[Browser] = []

    def start(flags: tuple[str, ...]) -> Browser:
        browser = accepted(
            Browser.open(tmp_path / f"browser-{len(browsers)}", media, flags)
        )
        browsers.append(browser)
        return browser

    try:
        yield start
    finally:
        for browser in reversed(browsers):
            _ = accepted(browser.close())


@pytest.fixture
def bridge_service(
    kernel_owner: Path, browser_binaries: tuple[Path, Path], tmp_path: Path
) -> Generator[Running]:
    running = accepted(verify(browser_binaries))
    lease = (
        accepted(Lease.borrow(running))
        if running is not None
        else accepted(
            Lease.start(
                browser_binaries[0], kernel_owner, ENDPOINT, tmp_path / "service.log"
            )
        )
    )
    try:
        confirmed = accepted(verify(browser_binaries))
        assert confirmed is not None and confirmed.identity == lease.running.identity
        yield lease.running
    finally:
        _ = accepted(lease.close())


@pytest.fixture(scope="session")
def browser_binaries(executable: Path) -> tuple[Path, Path]:
    root = Path(__file__).resolve().parents[2]
    return executable, accepted(release_binary(root))
