"""module: browser; responsibility: own one background Chromium playback session.

operations: Browser open/pid/alive/window/tab/control/player/url/close;
entities: Browser, _Lease, _Target; tests: browser_tests, browser functional cases.
No service supervision, utilization sampling or performance controls live here.
"""

import errno
import json
import math
import os
import signal
import subprocess
import sys
import time
from http.client import HTTPConnection, HTTPException
from pathlib import Path
from typing import BinaryIO
from urllib.parse import quote

from pydantic import BaseModel, TypeAdapter, ValidationError

from tests.support import browser_launch
from tests.support.browser.site import Site
from tests.support.cdp import Cdp
from tests.support.outcome import Ok, Rejected, Result
from tests.support.player import Player
from tests.support.window import Window

_CHROMIUM = Path("/usr/lib/chromium/chromium")


def _alive(process: subprocess.Popen[bytes] | None) -> bool:
    if process is None or process.returncode is not None:
        return False
    # Keep the session leader unreaped until all group signals are issued.
    # Reaping while observing liveness would permit its PID/PGID to be reused.
    return (
        os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is None
    )


def _stop(process: subprocess.Popen[bytes]) -> Result[None]:
    if process.returncode is not None:
        return Ok(None)
    failure: Rejected | None = None
    group_present = True
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        group_present = False
    except OSError as error:
        failure = Rejected(error.errno or 0, f"stop owned browser: {error}")
    if group_present:
        deadline = time.monotonic() + 5
        delay = 0.001
        while _alive(process):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            time.sleep(min(delay, remaining))
            delay = min(2 * delay, 0.05)
        forced = _alive(process)
        # The leader can exit while descendants ignore SIGTERM. Complete the
        # group signal plan while its unreaped leader still owns the PGID.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except OSError as error:
            failure = Rejected(error.errno or 0, f"kill owned browser group: {error}")
        if forced and failure is None:
            failure = Rejected(errno.ETIMEDOUT, "owned browser required SIGKILL")
    # Only terminal status permits reaping/releasing the leader. A second
    # observation timeout after SIGKILL would not establish termination.
    try:
        _ = process.wait()
    except OSError as error:
        failure = Rejected(error.errno or 0, f"wait for owned browser: {error}")
    return failure if failure is not None else Ok(None)


class _Target(BaseModel):
    id: str
    type: str
    webSocketDebuggerUrl: str


class _Lease:
    def __init__(self) -> None:
        self.site: Site | None = None
        self.log: BinaryIO | None = None
        self.process: subprocess.Popen[bytes] | None = None
        self.control: Cdp | None = None
        self.tab: Cdp | None = None
        self.window: Window | None = None
        self.closed: bool = False

    def close(self) -> Result[None]:
        if self.closed:
            return Ok(None)
        failure: Rejected | None = None
        try:
            if (
                self.window is not None
                and self.process is not None
                and _alive(self.process)
            ):
                result = self.window.close()
                if isinstance(result, Rejected):
                    failure = result
        finally:
            for channel in (self.tab, self.control):
                if channel is not None:
                    channel.close()
            if self.process is not None:
                stopped = _stop(self.process)
                if isinstance(stopped, Rejected):
                    failure = stopped
            if self.site is not None:
                stopped = self.site.close()
                if isinstance(stopped, Rejected):
                    failure = stopped
            if self.log is not None:
                self.log.close()
            self.closed = True
        return failure if failure is not None else Ok(None)


class Browser:
    def __init__(self, lease: _Lease) -> None:
        self._lease: _Lease = lease

    @classmethod
    def open(
        cls,
        directory: Path,
        media: tuple[Path, ...],
        flags: tuple[str, ...],
        *,
        executable: Path = _CHROMIUM,
        startup_timeout: float = 15,
    ) -> Result[Browser]:
        if not math.isfinite(startup_timeout) or startup_timeout <= 0:
            return Rejected(errno.EINVAL, "invalid browser startup deadline")
        lease = _Lease()
        result: Result[Browser] | None = None
        try:
            result = _open(directory, media, flags, executable, startup_timeout, lease)
        finally:
            if result is None or isinstance(result, Rejected):
                closed = lease.close()
                if isinstance(closed, Rejected):
                    if isinstance(result, Rejected):
                        result = Rejected(
                            result.code,
                            result.operation + "; cleanup: " + closed.operation,
                        )
                    else:
                        print(closed.operation, file=sys.stderr)
        assert result is not None
        return result

    @property
    def pid(self) -> int:
        assert self._lease.process is not None
        return self._lease.process.pid

    @property
    def alive(self) -> bool:
        return _alive(self._lease.process)

    @property
    def window(self) -> Window:
        assert self._lease.window is not None
        return self._lease.window

    @property
    def tab(self) -> Cdp:
        assert self._lease.tab is not None
        return self._lease.tab

    @property
    def control(self) -> Cdp:
        assert self._lease.control is not None
        return self._lease.control

    @property
    def player(self) -> Player:
        return Player(self.tab)

    def url(self, file: str) -> str:
        assert self._lease.site is not None
        return self._lease.site.origin + "/" + quote(file)

    def close(self) -> Result[None]:
        return self._lease.close()


def _open(
    directory: Path,
    media: tuple[Path, ...],
    flags: tuple[str, ...],
    executable: Path,
    timeout: float,
    lease: _Lease,
) -> Result[Browser]:
    try:
        directory.mkdir(mode=0o700)
        lease.log = (directory / "chromium.log").open("wb")
    except OSError as error:
        return Rejected(error.errno or 0, f"create browser workspace: {error}")
    site = Site.open(directory / "http", media)
    if isinstance(site, Rejected):
        return site
    lease.site = site.value
    launched = browser_launch.launch(
        directory / "profile", lease.log, flags, executable
    )
    if isinstance(launched, Rejected):
        return launched
    lease.process = launched.value
    endpoint = directory / "profile/DevToolsActivePort"
    deadline = time.monotonic() + timeout
    while (
        not endpoint.exists() and _alive(lease.process) and time.monotonic() < deadline
    ):
        time.sleep(0.02)
    if not endpoint.exists():
        return Rejected(
            errno.ETIMEDOUT,
            f"Chromium did not publish CDP; see {directory / 'chromium.log'}",
        )
    try:
        lines = endpoint.read_text().splitlines()
    except OSError as error:
        return Rejected(error.errno or 0, f"read browser CDP endpoint: {error}")
    if len(lines) != 2:
        return Rejected(errno.EPROTO, "invalid browser CDP endpoint")
    try:
        port = int(lines[0])
    except ValueError:
        return Rejected(errno.EPROTO, "invalid browser CDP port")
    if not 0 < port <= 65535:
        return Rejected(errno.EPROTO, "browser CDP port out of range")
    control = Cdp.open(f"ws://127.0.0.1:{port}" + lines[1])
    if isinstance(control, Rejected):
        return control
    lease.control = control.value
    window = Window.open(control.value)
    if isinstance(window, Rejected):
        return window
    lease.window = window.value
    connection = HTTPConnection("127.0.0.1", port, timeout=5)
    try:
        connection.request("GET", "/json/list")
        response = connection.getresponse()
        if response.status != 200:
            return Rejected(errno.EPROTO, "browser CDP target enumeration failed")
        try:
            targets = TypeAdapter(list[_Target]).validate_json(
                response.read(), strict=True
            )
        except ValidationError as error:
            return Rejected(errno.EPROTO, str(error))
    except (OSError, HTTPException) as error:
        return Rejected(errno.EIO, f"enumerate browser targets: {error}")
    finally:
        connection.close()
    pages = [
        target
        for target in targets
        if target.type == "page" and target.id == window.value.target
    ]
    if len(pages) != 1:
        return Rejected(errno.EPROTO, "owned browser page is missing or ambiguous")
    tab = Cdp.open(pages[0].webSocketDebuggerUrl)
    if isinstance(tab, Rejected):
        return tab
    lease.tab = tab.value
    info = control.value.call("SystemInfo.getInfo")
    if isinstance(info, Rejected):
        return info
    try:
        _ = (directory / "gpu-info.json").write_text(json.dumps(info.value, indent=2))
    except OSError as error:
        return Rejected(error.errno or 0, f"save browser graphics evidence: {error}")
    enabled = tab.value.call("Media.enable")
    if isinstance(enabled, Rejected):
        return enabled
    navigated = tab.value.call("Page.navigate", {"url": site.value.origin + "/"})
    if isinstance(navigated, Rejected):
        return navigated
    while time.monotonic() < deadline:
        state = tab.value.evaluate("document.readyState", str)
        if isinstance(state, Rejected):
            # Page.navigate can commit between Runtime.evaluate admission and
            # execution. The replacement document will provide a new context.
            if state.code != -32000 or state.operation not in (
                "Inspected target navigated or closed",
                "Execution context was destroyed.",
                "Cannot find context with specified id",
            ):
                return state
        elif state.value == "complete":
            return Ok(Browser(lease))
        time.sleep(0.02)
    return Rejected(errno.ETIMEDOUT, "owned browser page did not finish loading")
