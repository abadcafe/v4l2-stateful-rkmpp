"""module: tests.kernel.test_lifecycle; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_stream_off_waits_for_new_epoch_reset_acknowledgement
    test_close_video_emits_close_event
    test_disconnect_service_poisoned_video_reports_eio


"""

import errno
import time
from concurrent.futures import ThreadPoolExecutor

from tests.support.bridge_abi import Event, Message, Reply
from tests.support.kernel_peer import Peer
from tests.support.outcome import Rejected, accepted
from tests.support.queue import Queue, QueueType


def test_stream_off_waits_for_new_epoch_reset_acknowledgement(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    session, packets = packet_session
    output = Queue(session.video.fd, QueueType.OUTPUT)
    with ThreadPoolExecutor(max_workers=1) as executor:
        stopped = executor.submit(output.stream_off)
        deadline = time.monotonic() + 5
        while True:
            batch = accepted(session.service.exchange())
            if batch.event_count:
                break
            assert time.monotonic() < deadline
            time.sleep(0.001)
        reset = Message.from_buffer_copy(batch.events)
        assert reset.kind == Event.RESET and reset.epoch > packets[0].epoch
        assert not stopped.done()
        stale = Message.from_buffer_copy(packets[0])
        stale.kind = Reply.INPUT_DONE
        acknowledgement = Message.from_buffer_copy(reset)
        acknowledgement.kind = Reply.STOPPED
        replies = accepted(session.service.exchange([stale, acknowledgement]))
        assert list(replies.results[:2]) == [-errno.ESTALE, 0]
        accepted(stopped.result(timeout=5))


def test_close_video_emits_close_event(raw_session: Peer) -> None:
    raw_session.video.close()
    batch = accepted(raw_session.service.exchange())
    assert (
        batch.event_count == 1
        and Message.from_buffer_copy(batch.events).kind == Event.CLOSE
    )


def test_disconnect_service_poisoned_video_reports_eio(raw_session: Peer) -> None:
    raw_session.service.close()
    result = Queue(raw_session.video.fd, QueueType.OUTPUT).request(2)
    assert isinstance(result, Rejected) and result.code == errno.EIO


def test_stream_off_disconnected_service_unblocks_without_reset_timeout(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    peer, _ = packet_session
    with ThreadPoolExecutor(max_workers=1) as executor:
        stopped = executor.submit(Queue(peer.video.fd, QueueType.OUTPUT).stream_off)
        deadline = time.monotonic() + 5
        while accepted(peer.service.exchange()).event_count == 0:
            assert time.monotonic() < deadline
            time.sleep(0.001)
        began = time.monotonic()
        peer.service.close()
        accepted(stopped.result(timeout=1))
        assert time.monotonic() - began < 1
    result = Queue(peer.video.fd, QueueType.OUTPUT).request(1)
    assert isinstance(result, Rejected) and result.code == errno.EIO


def test_stream_off_missing_acknowledgement_poisoning_is_bounded(
    packet_session: tuple[Peer, tuple[Message, ...]],
) -> None:
    peer, _ = packet_session
    output = Queue(peer.video.fd, QueueType.OUTPUT)
    began = time.monotonic()
    accepted(output.stream_off())
    assert 4.5 <= time.monotonic() - began < 8
    result = output.request(1)
    assert isinstance(result, Rejected) and result.code == errno.EIO


def test_close_session_rejects_further_exchange_and_registration(
    raw_session: Peer,
) -> None:
    raw_session.video.close()
    assert accepted(raw_session.service.exchange()).event_count == 1
    for result in (
        raw_session.service.exchange(),
        raw_session.service.register(10, 1, 1),
    ):
        assert isinstance(result, Rejected) and result.code == errno.EPIPE
