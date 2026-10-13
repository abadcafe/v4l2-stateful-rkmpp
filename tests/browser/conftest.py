"""module: browser fixtures; responsibility: lease functional acceptance sessions.

operations: browser_factory, bridge_service;
entities: BrowserFactory; tests: test_graphics, test_playback, test_sessions.
Frequency policy and utilization sampling belong exclusively to tests/tools.
"""

import json
from collections.abc import Callable, Generator
from pathlib import Path

import pytest

from tests.support.browser import Browser
from tests.support.build_identity import existing_service_binary
from tests.support.outcome import accepted
from tests.support.service_process import Lease
from tests.support.service_runtime import (
    ENDPOINT,
    Running,
    locate,
    require_idle,
    sample,
)

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
        _ = (tmp_path / f"graphics-{len(browsers)}.json").write_text(
            json.dumps(accepted(browser.control.call("SystemInfo.getInfo")), indent=2)
        )
        return browser

    try:
        yield start
    finally:
        for browser in reversed(browsers):
            _ = accepted(browser.close())


@pytest.fixture
def bridge_service(kernel_owner: Path, tmp_path: Path) -> Generator[Running]:
    accepted(require_idle(control=kernel_owner))
    lease = accepted(
        Lease.start(
            accepted(existing_service_binary(Path(__file__).resolve().parents[2])),
            kernel_owner,
            ENDPOINT,
            tmp_path / "service.log",
        )
    )
    try:
        confirmed = accepted(locate())
        assert confirmed is not None and confirmed.identity == lease.running.identity
        _ = (tmp_path / "service-identity.json").write_text(
            lease.running.identity.model_dump_json(indent=2)
        )
        yield lease.running
    finally:
        _ = accepted(lease.close())
