"""module: tests.support.bridge_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_exchange_and_register_non_session_fd_report_enotty


"""

import errno
from pathlib import Path

from tests.support.bridge import Session
from tests.support.outcome import Rejected, accepted
from tests.support.syscalls import ioctl, open_fd


def test_exchange_and_register_non_session_fd_report_enotty() -> None:
    owner = accepted(open_fd(Path("/dev/null")))
    session = Session(owner)
    fd = owner.fd
    for result in (session.exchange(), session.register(10, 1, 1)):
        assert isinstance(result, Rejected) and result.code == errno.ENOTTY
    session.close()
    result = ioctl(fd, 0)
    assert isinstance(result, Rejected) and result.code == errno.EBADF
