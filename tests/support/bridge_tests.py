"""module: tests.support.bridge_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_exchange_and_register_non_session_fd_report_enotty


"""

import errno
from contextlib import ExitStack
from pathlib import Path

from tests.support.bridge import Session
from tests.support.outcome import Rejected, accepted
from tests.support.syscalls import ioctl, open_fd


def test_session_context_releases_all_acquired_endpoints_before_early_return() -> None:
    descriptors: list[int] = []

    def acquire() -> None:
        with ExitStack() as owned:
            sessions: list[Session] = []
            for _ in range(3):
                session = owned.enter_context(
                    Session(accepted(open_fd(Path("/dev/null"))))
                )
                descriptors.append(session.fd)
                sessions.append(session)
            result = sessions[0].exchange()
            assert isinstance(result, Rejected) and result.code == errno.ENOTTY
            return

    acquire()
    assert len(descriptors) == 3
    for descriptor in descriptors:
        result = ioctl(descriptor, 0)
        assert isinstance(result, Rejected) and result.code == errno.EBADF


def test_exchange_and_register_non_session_fd_report_enotty() -> None:
    owner = accepted(open_fd(Path("/dev/null")))
    session = Session(owner)
    fd = owner.fd
    for result in (session.exchange(), session.register(10, 1, 1)):
        assert isinstance(result, Rejected) and result.code == errno.ENOTTY
    session.close()
    result = ioctl(fd, 0)
    assert isinstance(result, Rejected) and result.code == errno.EBADF
