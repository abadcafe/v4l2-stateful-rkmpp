"""module: test_plane_storage; responsibility: specify two-plane kernel DMA storage.

operations: pytest cases; entities: none;
tests: public P010 layout exposes independent planes sharing a parent allocation.
This kernel capability does not imply Chromium can display P010.
"""

import ctypes
import mmap

from tests.support.bridge_abi import VERSION, Message, Reply
from tests.support.buffer_abi import Plane, VideoBuffer
from tests.support.dmabuf import read_bytes
from tests.support.kernel_peer import Peer
from tests.support.outcome import accepted
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import command, ioctl
from tests.support.video_formats import get_format


def test_layout_p010_two_planes_export_and_map_their_declared_regions(
    raw_session: Peer,
) -> None:
    layout = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        epoch=1,
        width=128,
        height=64,
        stride=256,
        rows=64,
        size=24576,
        format=2,
    )
    assert accepted(raw_session.service.exchange([layout])).results[0] == 0
    format = accepted(get_format(raw_session.video.fd, QueueType.CAPTURE))
    assert (format.planes, format.strides, format.sizes) == (
        2,
        (256, 256),
        (16384, 8192),
    )
    capture = Queue(raw_session.video.fd, QueueType.CAPTURE)
    assert accepted(capture.request(1)) == 18
    planes = (Plane * 2)()
    payload = VideoBuffer(kind=9, memory=1, length=2)
    payload.location.planes = ctypes.addressof(planes)
    assert (
        accepted(
            ioctl(
                raw_session.video.fd,
                command(3, "V", 9, ctypes.sizeof(payload)),
                payload,
            )
        )
        == 0
    )
    y, uv = Plane.from_buffer(planes), Plane.from_buffer(planes, ctypes.sizeof(Plane))
    assert (y.length, uv.length) == (16384, 8192)
    assert y.memory.offset != uv.memory.offset and y.memory.offset >= 1 << 30
    with (
        accepted(capture.export(0, 0)) as y_fd,
        accepted(capture.export(0, 1)) as uv_fd,
    ):
        with (
            mmap.mmap(
                raw_session.video.fd,
                y.length,
                flags=mmap.MAP_SHARED,
                offset=y.memory.offset,
            ) as y_map,
            mmap.mmap(
                raw_session.video.fd,
                uv.length,
                flags=mmap.MAP_SHARED,
                offset=uv.memory.offset,
            ) as uv_map,
        ):
            y_map[:16], uv_map[:16] = b"Y" * 16, b"U" * 16
            assert accepted(read_bytes(y_fd.fd, 16)) == b"Y" * 16
            assert accepted(read_bytes(uv_fd.fd, 16)) == b"U" * 16
            assert accepted(read_bytes(y_fd.fd, 16, 16384)) == b"U" * 16
    assert accepted(capture.request(0)) == 0
