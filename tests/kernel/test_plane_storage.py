"""module: test_plane_storage; responsibility: specify contiguous P010 storage.

operations: pytest cases; entities: none; tests: layout, export, mapping lifetime.
"""

from rkmpp_bridge_uapi import VERSION, Message, Reply

from tests.support.dmabuf import read_bytes
from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted, present
from tests.support.queue import Queue, QueueType
from tests.support.video_formats import get_format


def test_layout_p010_exports_contiguous_luma_and_chroma(raw_session: Peer) -> None:
    layout = Message(
        coded_width=128,
        coded_height=64,
        version=VERSION,
        kind=Reply.LAYOUT,
        count=2,
        epoch=1,
        width=128,
        height=64,
        stride=256,
        rows=64,
        size=24576,
        format=2,
    )
    assert present(accepted(raw_session.service.exchange([layout])).result(0)) == 0
    format = accepted(get_format(raw_session.video.fd, QueueType.CAPTURE))
    assert (format.planes, format.strides, format.sizes) == (1, (256,), (24576,))
    capture = Queue(raw_session.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(1)) == 2
    assert not accepted(capture.query(0)).flags & 1
    with accepted(capture.map(0)) as mapping:
        assert accepted(capture.query(0)).flags & 1
        mapping[:16] = b"Y" * 16
        mapping[16384:16400] = b"U" * 16
        with accepted(capture.export(0)) as fd:
            assert accepted(read_bytes(fd.fd, 16)) == b"Y" * 16
            assert accepted(read_bytes(fd.fd, 16, 16384)) == b"U" * 16
    assert not accepted(capture.query(0)).flags & 1
    assert accepted(capture.request(0)) == 0
