"""module: test_protocol_validation; responsibility: reject invalid service commands.

operations: pytest cases; entities: none; tests: event headers, reserved fields
and command domains. Rejection precedes native resource transfer.
"""

import errno

import pytest
from rkmpp_bridge_uapi import EXCHANGE, VERSION, Batch, Event, Message, Reply

from tests.support.mock_process import Process
from tests.support.mock_wire import Reply as Response
from tests.support.mock_wire import Request
from tests.support.outcome import accepted, present


def closed(request: Request) -> Response:
    assert request.command == EXCHANGE
    batch = present(Batch.decode(request.body))
    batch.event_count = 1
    assert batch.set_event(0, Message(version=VERSION, kind=Event.CLOSE, epoch=1))
    return Response(body=bytes(batch))


@pytest.mark.parametrize(
    ("kind", "version", "flags"),
    [
        (999, VERSION, 0),
        (Event.PACKET, VERSION, 0),
        (Event.RECYCLE, VERSION, 0),
        (Event.CLOSE, VERSION + 1, 0),
    ],
    ids=[
        "unknown",
        "zero-packet-pool",
        "zero-recycle-pool",
        "event-version",
    ],
)
def test_exchange_invalid_event_returns_failed_without_killing_other_sessions(
    mock_service: Process,
    kind: int,
    version: int,
    flags: int,
) -> None:
    first = True
    replies: list[int] = []

    def handle(request: Request) -> Response:
        nonlocal first
        assert request.command == EXCHANGE
        batch = present(Batch.decode(request.body))
        if first:
            first = False
            batch.event_count = 1
            assert batch.set_event(
                0, Message(version=version, kind=kind, flags=flags, epoch=1, count=1)
            )
            return Response(body=bytes(batch))
        replies.extend(present(batch.reply(i)).kind for i in range(batch.reply_count))
        return Response(code=errno.EPIPE)

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert replies == [Reply.FAILED]
    accepted(accepted(mock_service.session(closed)).wait(5))
    assert mock_service.process.poll() is None


@pytest.mark.parametrize("kind", list(Event))
@pytest.mark.parametrize("reserved", [1, 0xFFFFFFFF])
def test_event_reserved_is_rejected_before_resource_transfer_or_state_mutation(
    mock_service: Process, kind: Event, reserved: int
) -> None:
    first = True
    responses: list[Message] = []

    def handle(request: Request) -> Response:
        nonlocal first
        # Rejection must precede REGISTER_POOL, CLOSE, or backend RESET.
        assert request.command == EXCHANGE
        batch = present(Batch.decode(request.body))
        if first:
            first = False
            batch.event_count = 1
            message = Message(
                version=VERSION, kind=kind, epoch=2, generation=3, pool_id=7
            )
            message.reserved = reserved
            assert batch.set_event(0, message)
            return Response(body=bytes(batch))
        responses.extend(present(batch.reply(i)) for i in range(batch.reply_count))
        return Response(code=errno.EPIPE)

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert len(responses) == 1
    failed = responses[0]
    assert (failed.kind, failed.epoch, failed.generation) == (Reply.FAILED, 2, 3)
    assert failed.reserved == 0
    accepted(accepted(mock_service.session(closed)).wait(5))
    assert mock_service.process.poll() is None


@pytest.mark.parametrize(
    ("kind", "invalid"),
    [
        (Event.PACKET, {"codec": 0}),
        (Event.PACKET, {"index": 32}),
        (Event.PACKET, {"count": 0}),
        (Event.PACKET, {"data_offset": 0xFFFFFFFF}),
        (Event.PACKET, {"flags": 0x20000}),
        (Event.CAPTURE, {"pool_id": 0}),
        (Event.CAPTURE, {"count": 33}),
        (Event.CAPTURE, {"flags": 2}),
        (Event.RECYCLE, {"index": 32}),
        (Event.RECYCLE, {"count": 0}),
        (Event.RESET, {"count": 0}),
        (Event.RESET, {"flags": 1}),
    ],
)
def test_command_invalid_domain_is_rejected_before_pool_transfer(
    mock_service: Process, kind: int, invalid: dict[str, int]
) -> None:
    first = True
    replies: list[int] = []

    def handle(request: Request) -> Response:
        nonlocal first
        # REGISTER_POOL before validation would transfer resources for an
        # invalid command; only the EXCHANGE failure acknowledgement is legal.
        assert request.command == EXCHANGE
        batch = present(Batch.decode(request.body))
        if first:
            first = False
            fields = {
                "version": VERSION,
                "kind": kind,
                "epoch": 1,
                "pool_id": 1,
                "count": 1,
                "bytesused": 1,
                "codec": int.from_bytes(b"H264", "little"),
            }
            fields.update(invalid)
            batch.event_count = 1
            assert batch.set_event(0, Message(**fields))
            return Response(body=bytes(batch))
        replies.extend(
            present(batch.reply(index)).kind for index in range(batch.reply_count)
        )
        return Response(code=errno.EPIPE)

    accepted(accepted(mock_service.session(handle)).wait(5))
    assert replies == [Reply.FAILED]
    assert mock_service.process.poll() is None
