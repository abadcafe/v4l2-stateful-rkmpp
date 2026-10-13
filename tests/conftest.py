"""module: acceptance fixtures; responsibility: own process acceptance lifetimes.

operations: artifact/executable/device/process fixtures; entities: PeerFactory;
tests: tests/kernel, tests/service, tests/browser, tests/support.

External contracts only: compiler CLI, sysfs, /proc, and device nodes.
"""

import os
import threading
from collections.abc import Callable, Generator
from contextlib import ExitStack
from pathlib import Path

import pytest
from websockets.sync.server import ServerConnection, serve

from tests.support.build_identity import service_binary
from tests.support.kernel_peer import Peer
from tests.support.kernel_runtime import exclusive, verify
from tests.support.outcome import accepted
from tests.support.video import Video

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture(scope="session")
def artifacts() -> Path:
    return ROOT / "tests/artifacts"


@pytest.fixture(scope="session")
def executable() -> Path:
    return accepted(service_binary(ROOT))


@pytest.fixture(scope="session")
def kernel_owner(tmp_path_factory: pytest.TempPathFactory) -> Path:
    control = accepted(verify())
    directory = tmp_path_factory.mktemp("kernel-runtime")
    _ = (directory / "kernel-release.txt").write_text(os.uname().release)
    note = Path("/sys/module/v4l2_rkmpp_bridge/notes/.note.gnu.build-id")
    try:
        identity = note.read_bytes().hex()
    except OSError as error:
        identity = f"Unavailable: {error}"
    _ = (directory / "build-id-note.txt").write_text(identity)
    assert control.is_char_device(), (
        f"Loaded module is missing control device {control}"
    )
    assert Path("/dev/video0").is_char_device(), (
        "Loaded module is missing video device /dev/video0"
    )
    return control


@pytest.fixture(scope="session")
def kernel_idle(kernel_owner: Path) -> Path:
    accepted(exclusive(kernel_owner))
    return kernel_owner


@pytest.fixture
def video(kernel_idle: Path) -> Generator[Video]:
    with accepted(Peer.open(kernel_idle, Path("/dev/video0"))) as peer:
        yield peer.video


type PeerFactory = Callable[[Callable[[ServerConnection], None]], str]


@pytest.fixture
def peer() -> Generator[PeerFactory]:
    threads: list[threading.Thread] = []
    with ExitStack() as stack:

        def start(handler: Callable[[ServerConnection], None]) -> str:
            server = stack.enter_context(
                serve(handler, "127.0.0.1", 0, close_timeout=1)
            )
            thread = threading.Thread(target=server.serve_forever)
            threads.append(thread)
            thread.start()
            return f"ws://127.0.0.1:{server.socket.getsockname()[1]}"

        yield start
    for thread in threads:
        thread.join(timeout=5)
        assert not thread.is_alive()
