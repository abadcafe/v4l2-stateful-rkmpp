"""module: test_reply_validation; responsibility: specify reply flag admission.

operations: pytest cases; entities: none; tests: malformed flags cannot mutate
session state, complete buffers, or suppress subsequent valid replies.
"""

import errno

import pytest
from rkmpp_bridge_uapi import VERSION, Message, Reply

from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted, present
from tests.support.queue import QueueType
from tests.support.video_formats import get_format


@pytest.mark.parametrize("kind", list(Reply))
def test_reply_unknown_flags_reject_before_state_admission(
    raw_session: Peer, kind: Reply
) -> None:
    allowed = 2 if kind == Reply.INPUT_DONE else 0x10002 if kind == Reply.FRAME else 0
    for bit in range(32):
        flags = 1 << bit
        if flags & allowed:
            continue
        message = Message(
            version=VERSION,
            kind=kind,
            epoch=1,
            flags=flags,
            width=128,
            height=64,
            coded_width=128,
            coded_height=64,
            stride=128,
            rows=64,
            size=12288,
            format=1,
            count=18,
        )
        assert present(accepted(raw_session.service.exchange([message])).result(0)) == (
            -errno.EINVAL
        )
    assert accepted(get_format(raw_session.video.fd, QueueType.CAPTURE)).width == 64
    assert accepted(raw_session.service.exchange()).event_count == 0
