"""module: test_plane_storage; responsibility: specify contiguous NV12 storage.

operations: pytest cases; entities: none; tests: layout, export, mapping lifetime.
"""

import errno

from rkmpp_bridge_uapi import VERSION, Message, Reply

from tests.support.dmabuf import read_bytes
from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted, present
from tests.support.queue import Queue, QueueType
from tests.support.video_formats import enumerate_formats, get_format


def test_layout_rejects_partial_chroma_pairs_without_publication(
    raw_session: Peer,
) -> None:
    accepted(raw_session.video.subscribe(5))
    layout = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        epoch=1,
        count=2,
        width=128,
        height=64,
        coded_width=128,
        coded_height=64,
        stride=129,
        rows=64,
        size=129 * 64 * 3 // 2,
        format=1,
    )
    assert present(accepted(raw_session.service.exchange([layout])).result(0)) == (
        -errno.EINVAL
    )
    assert accepted(raw_session.video.events()) == []
    assert accepted(get_format(raw_session.video.fd, QueueType.CAPTURE)).strides == (
        64,
    )
    layout.stride = 130
    layout.size = 130 * 64 * 3 // 2
    assert present(accepted(raw_session.service.exchange([layout])).result(0)) == 0
    actual = accepted(get_format(raw_session.video.fd, QueueType.CAPTURE))
    assert (actual.width, actual.strides) == (130, (130,))


def test_layout_nv12_exports_contiguous_luma_and_chroma(raw_session: Peer) -> None:
    layout = Message(
        coded_width=128,
        coded_height=64,
        version=VERSION,
        kind=Reply.LAYOUT,
        count=2,
        epoch=1,
        width=128,
        height=64,
        stride=128,
        rows=64,
        size=12288,
        format=1,
    )
    assert present(accepted(raw_session.service.exchange([layout])).result(0)) == 0
    format = accepted(get_format(raw_session.video.fd, QueueType.CAPTURE))
    assert format.fourcc == int.from_bytes(b"NV12", "little")
    assert accepted(enumerate_formats(raw_session.video.fd, QueueType.CAPTURE)) == (
        [format.fourcc]
    )
    assert (format.planes, format.strides, format.sizes) == (1, (128,), (12288,))
    capture = Queue(raw_session.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(1)) == 2
    assert not accepted(capture.query(0)).flags & 1
    with accepted(capture.map(0)) as mapping:
        assert accepted(capture.query(0)).flags & 1
        mapping[:16] = b"Y" * 16
        mapping[8192:8208] = b"U" * 16
        with accepted(capture.export(0)) as fd:
            assert accepted(read_bytes(fd.fd, 16)) == b"Y" * 16
            assert accepted(read_bytes(fd.fd, 16, 8192)) == b"U" * 16
    assert not accepted(capture.query(0)).flags & 1
    assert accepted(capture.request(0)) == 0
