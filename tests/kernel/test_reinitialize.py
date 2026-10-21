"""module: test_reinitialize; responsibility: define failed-incarnation recovery.

operations: pytest cases; entities: none; tests: partial/full cleanup,
new endpoint ownership, stale replies/registration/release, repeated failures.
V4L2 stateful Decoding permits initialization on the same fd after both queues
are stopped and every buffer is released.
"""

import ctypes
import errno
from contextlib import ExitStack
from pathlib import Path

import pytest
from rkmpp_bridge_uapi import ACCEPT, VERSION, Message, Reply

from tests.support.bridge import Session
from tests.support.control_abi import Control
from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted, present
from tests.support.queue import Queue, QueueType
from tests.support.stream import Codec
from tests.support.syscalls import OwnedFd, command, ioctl, open_fd
from tests.support.video import Video
from tests.support.video_formats import set_format


def test_full_cleanup_restores_capture_minimum_without_setting_format(
    raw_session: Peer,
) -> None:
    layout = Message(
        version=VERSION,
        kind=Reply.LAYOUT,
        epoch=1,
        count=32,
        width=64,
        height=64,
        coded_width=64,
        coded_height=64,
        stride=64,
        rows=64,
        size=6144,
        format=1,
    )
    assert present(accepted(raw_session.service.exchange([layout])).result(0)) == 0
    minimum = Control(id=0x980927)
    request = command(3, "V", 27, ctypes.sizeof(minimum))
    _ = accepted(ioctl(raw_session.video.fd, request, minimum))
    assert minimum.value == 32
    raw_session.service.close()
    for kind in (QueueType.OUTPUT, QueueType.CAPTURE):
        assert accepted(Queue(raw_session.video.fd, kind).request(0)) == 0
    _ = accepted(ioctl(raw_session.video.fd, request, minimum))
    assert minimum.value == 2
    assert accepted(Queue(raw_session.video.fd, QueueType.CAPTURE).request(1)) == 2


@pytest.mark.parametrize("capture_first", [False, True])
@pytest.mark.parametrize("disconnect", [False, True])
def test_full_cleanup_reinitializes_same_fd_with_isolated_service_endpoint(
    kernel_owner: Path, capture_first: bool, disconnect: bool
) -> None:
    with ExitStack() as owned:
        broker = owned.enter_context(accepted(open_fd(kernel_owner)))
        video = owned.enter_context(accepted(Video.open(Path("/dev/video0"))))
        service = Session(OwnedFd(accepted(ioctl(broker.fd, ACCEPT))))
        _ = owned.callback(service.close)
        accepted(video.subscribe(5))
        output = Queue(video.fd, QueueType.OUTPUT)
        capture = Queue(video.fd, QueueType.CAPTURE)
        stale: list[Session] = []
        for cycle in range(2):
            _ = accepted(
                set_format(video.fd, QueueType.OUTPUT, Codec.H264.fourcc, size=4096)
            )
            assert accepted(output.request(1)) == 1
            assert accepted(capture.request(18)) == 18
            accepted(output.queue(0, 16))
            for index in range(18):
                accepted(capture.queue(index))
            accepted(output.stream_on())
            accepted(capture.stream_on())
            batch = accepted(service.exchange())
            packet = present(batch.event(0))
            while accepted(service.exchange()).event_count:
                pass
            layout = Message(
                version=VERSION,
                kind=Reply.LAYOUT,
                count=18,
                epoch=packet.epoch,
                width=128,
                height=64,
                coded_width=128,
                coded_height=64,
                stride=128,
                rows=64,
                size=12288,
                format=1,
            )
            assert present(accepted(service.exchange([layout])).result(0)) == 0
            # Leave SOURCE_CHANGE queued. Recovery must preserve subscription,
            # but an event from the failed incarnation cannot survive teardown.
            if disconnect:
                service.close()
            else:
                failed = Message(version=VERSION, kind=Reply.FAILED, epoch=packet.epoch)
                result = service.exchange([failed])
                assert isinstance(result, Rejected) and result.code == errno.EPIPE
                stale.append(service)
            queues = (capture, output) if capture_first else (output, capture)
            for queue in queues:
                accepted(queue.stream_off())
            _ = accepted(queues[0].request(0))
            blocked = set_format(video.fd, QueueType.OUTPUT, Codec.H264.fourcc)
            assert isinstance(blocked, Rejected) and blocked.code == errno.EIO
            _ = accepted(queues[1].request(0))
            _ = accepted(
                set_format(video.fd, QueueType.OUTPUT, Codec.VP9.fourcc, size=8192)
            )
            service = Session(OwnedFd(accepted(ioctl(broker.fd, ACCEPT))))
            _ = owned.callback(service.close)
            assert accepted(service.exchange()).event_count == 0
            assert accepted(video.events()) == []
            assert accepted(output.request(1)) == 1
            accepted(output.queue(0, 24, 123000 + cycle))
            accepted(output.stream_on())
            batch = accepted(service.exchange())
            assert batch.event_count == 1
            current = present(batch.event(0))
            assert current.epoch > packet.epoch and current.codec == Codec.VP9.fourcc
            for previous in stale:
                forged = current.clone()
                forged.kind = Reply.INPUT_DONE
                result = previous.exchange([forged])
                assert isinstance(result, Rejected) and result.code == errno.EPIPE
                result = previous.register(
                    QueueType.OUTPUT, current.epoch, current.pool_id
                )
                assert isinstance(result, Rejected) and result.code == errno.EPIPE
                previous.close()
            stale.clear()
            done = current.clone()
            done.kind = Reply.INPUT_DONE
            assert present(accepted(service.exchange([done])).result(0)) == 0
            assert accepted(output.dequeue()).bytesused == (24,)
            # Fail the fresh incarnation too: the next iteration must not depend
            # on the original endpoint or any stale queue error/layout state.
            failed = Message(version=VERSION, kind=Reply.FAILED, epoch=current.epoch)
            result = service.exchange([failed])
            assert isinstance(result, Rejected) and result.code == errno.EPIPE
            service.close()
            accepted(output.stream_off())
            _ = accepted(output.request(0))
            _ = accepted(capture.request(0))
            service = Session(OwnedFd(accepted(ioctl(broker.fd, ACCEPT))))
            _ = owned.callback(service.close)
            assert accepted(service.exchange()).event_count == 0
