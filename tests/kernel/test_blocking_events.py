"""module: test_blocking_events; responsibility: specify blocking event progress.

operations: pytest cases; entities: none; tests: event waits release session ownership.
"""

import ctypes
import os
import signal
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from types import FrameType

from tests.support.bridge_abi import VERSION, Message, Reply
from tests.support.event_abi import Event
from tests.support.kernel_peer import Peer
from tests.support.outcome import Result, accepted
from tests.support.syscalls import command, ioctl


def _interrupt(_signum: int, _frame: FrameType | None) -> None:
    pass


def test_dqevent_wait_allows_service_to_publish_source_change(
    raw_session: Peer,
) -> None:
    accepted(raw_session.video.subscribe(5))
    os.set_blocking(raw_session.video.fd, True)
    entered = threading.Event()
    identity: list[int] = []
    event = Event()

    def receive() -> Result[int]:
        identity.append(threading.get_ident())
        entered.set()
        return ioctl(
            raw_session.video.fd, command(2, "V", 89, ctypes.sizeof(event)), event
        )

    previous = signal.signal(signal.SIGUSR1, _interrupt)
    try:
        with ThreadPoolExecutor(max_workers=2) as executor:
            waiting = executor.submit(receive)
            try:
                assert entered.wait(2)
                time.sleep(0.05)
                assert not waiting.done()
                publishing = executor.submit(
                    raw_session.service.exchange,
                    [
                        Message(
                            version=VERSION,
                            kind=Reply.LAYOUT,
                            epoch=1,
                            width=64,
                            height=64,
                            coded_width=64,
                            coded_height=64,
                            stride=64,
                            rows=64,
                            size=6144,
                            format=1,
                        )
                    ],
                )
                assert accepted(publishing.result(timeout=2)).results[0] == 0
                _ = accepted(waiting.result(timeout=2))
                assert event.kind == 5
            finally:
                if identity and not waiting.done():
                    signal.pthread_kill(identity[0], signal.SIGUSR1)
    finally:
        _ = signal.signal(signal.SIGUSR1, previous)
        os.set_blocking(raw_session.video.fd, False)
