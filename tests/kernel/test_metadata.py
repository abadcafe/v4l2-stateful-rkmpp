"""module: test_metadata; responsibility: specify V4L2 frame metadata propagation.

operations: pytest cases; entities: none; tests: fields and timestamp source.
"""

import ctypes

import pytest
from rkmpp_bridge_uapi import Reply

from tests.kernel.test_frame_reply import CaptureSession, frame

# Reuse the public session fixture, never implementation state.
from tests.kernel.test_frame_reply import capture_session as capture_session
from tests.support.buffer_abi import Plane, VideoBuffer
from tests.support.outcome import accepted, present
from tests.support.queue import Queue, QueueType
from tests.support.syscalls import command, ioctl


@pytest.mark.parametrize(
    "fields,expected",
    [
        ((3, 4, 3, 4), (0, 0, 1, 1)),
        ((4, 3, 4, 3), (0, 0, 1, 1)),
        ((3, 3, 4, 0), (0, 1, 1, 2)),
        ((3, 0, 4, 3), (0, 1, 2, 2)),
    ],
)
def test_capture_sequence_counts_frames_not_fields(
    capture_session: CaptureSession, fields: tuple[int, ...], expected: tuple[int, ...]
) -> None:
    session = capture_session
    actual: list[int] = []
    for index, field in enumerate(fields):
        reply = frame(session.queued[index])
        reply.field = field
        assert present(accepted(session.raw.service.exchange([reply])).result(0)) == 0
        actual.append(accepted(session.queue.dequeue()).sequence)
    assert tuple(actual) == expected


@pytest.mark.parametrize("source", [0, 0x10000])
def test_timestamp_source_survives_packet_and_frame(
    capture_session: CaptureSession, source: int
) -> None:
    session = capture_session
    output = Queue(session.raw.video.fd, QueueType.OUTPUT)
    # Retire the fixture's first packet and submit an explicitly marked one.
    done = session.packets[0].clone()
    done.kind = Reply.INPUT_DONE
    done.pool_id = 1
    assert present(accepted(session.raw.service.exchange([done])).result(0)) == 0
    _ = accepted(output.dequeue())
    planes = (Plane * 1)()
    Plane.from_buffer(planes).bytesused = 16
    payload = VideoBuffer(kind=10, memory=1, length=1, flags=source)
    payload.location.planes = ctypes.addressof(planes)
    _ = accepted(
        ioctl(
            session.raw.video.fd, command(3, "V", 15, ctypes.sizeof(payload)), payload
        )
    )
    packet = present(accepted(session.raw.service.exchange()).event(0))
    assert packet.flags & 0x70000 == source
    reply = frame(session.queued[0])
    reply.flags = source
    assert present(accepted(session.raw.service.exchange([reply])).result(0)) == 0
    assert accepted(session.queue.dequeue()).flags & 0x70000 == source
