"""module: browser_launch; responsibility: launch an owned background browser.

operations: launch, DECODE_FLAGS; entities: none; tests: browser_launch_tests.
The current desktop environment is inherited; activation tokens are removed.
"""

import os
import subprocess
from pathlib import Path
from typing import BinaryIO

from tests.support.outcome import Ok, Rejected, Result

DECODE_FLAGS = (
    "--enable-features=AcceleratedVideoDecoder,AcceleratedVideoDecodeLinuxGL,AcceleratedVideoDecodeLinuxZeroCopyGL",
    "--disable-features=ResolutionBasedDecoderPriority",
)

_CHROMIUM = Path("/usr/lib/chromium/chromium")


def launch(
    profile: Path,
    log: BinaryIO,
    flags: tuple[str, ...],
    executable: Path = _CHROMIUM,
) -> Result[subprocess.Popen[bytes]]:
    environment = dict(os.environ)
    for name in ("XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"):
        _ = environment.pop(name, None)
    try:
        profile.mkdir(mode=0o700)
        process = subprocess.Popen(
            [
                str(executable),
                "--user-data-dir=" + str(profile),
                "--remote-debugging-port=0",
                "--no-first-run",
                "--no-default-browser-check",
                "--no-startup-window",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-sync",
                "--disable-background-timer-throttling",
                "--disable-renderer-backgrounding",
                "--disable-backgrounding-occluded-windows",
                "--password-store=basic",
                "--enable-logging=stderr",
                *DECODE_FLAGS,
                *flags,
            ],
            env=environment,
            stdout=log,
            stderr=log,
            start_new_session=True,
        )
    except OSError as error:
        return Rejected(
            error.errno or 0, f"Start Chromium as the current user: {error}"
        )
    return Ok(process)
