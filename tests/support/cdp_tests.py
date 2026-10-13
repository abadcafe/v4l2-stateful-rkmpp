"""module: tests.support.cdp_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: Answer; tests:
    test_call_delivers_result_and_preserves_interleaved_notifications
    test_call_remote_failure_is_rejected
    test_call_invalid_wire_response_is_rejected
    test_call_timeout_is_bounded
    test_call_closed_peer_is_rejected
    test_open_invalid_websocket_uri_is_rejected
    test_call_reply_without_result_or_error_is_rejected
    test_call_string_response_id_is_rejected
    test_evaluate_validates_value_and_rejects_script_or_schema_errors

Public CDP operations against an owned WebSocket protocol peer.
"""

import errno
import json
import threading

import pytest
from pydantic import BaseModel, JsonValue, TypeAdapter
from websockets.sync.server import ServerConnection

from tests.conftest import PeerFactory
from tests.support.cdp import Cdp
from tests.support.outcome import Rejected, accepted


def test_call_delivers_result_and_preserves_interleaved_notifications(
    peer: PeerFactory,
) -> None:
    def handle(connection: ServerConnection) -> None:
        message = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        assert message["method"] == "Example.call" and message["params"] == {
            "number": 3
        }
        connection.send(
            json.dumps({"method": "Example.changed", "params": {"number": 4}})
        )
        connection.send(json.dumps({"id": message["id"], "result": {"done": True}}))

    with accepted(Cdp.open(peer(handle))) as cdp:
        assert accepted(cdp.call("Example.call", {"number": 3})) == {"done": True}
        events = cdp.events()
        assert (
            len(events) == 1
            and events[0].method == "Example.changed"
            and events[0].params == {"number": 4}
        )
        assert cdp.events() == ()


def test_call_remote_failure_is_rejected(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        message = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps(
                {
                    "id": message["id"],
                    "error": {"code": -32601, "message": "missing method"},
                }
            )
        )

    with accepted(Cdp.open(peer(handle))) as cdp:
        result = cdp.call("Missing.method")
        assert (
            isinstance(result, Rejected)
            and result.code == -32601
            and "missing method" in result.operation
        )


@pytest.mark.parametrize(
    "payload", ["not-json", '{"id":99,"result":{}}', '{"result":{}}']
)
def test_call_invalid_wire_response_is_rejected(
    peer: PeerFactory, payload: str
) -> None:
    def handle(connection: ServerConnection) -> None:
        _ = connection.recv()
        connection.send(payload)

    with accepted(Cdp.open(peer(handle))) as cdp:
        result = cdp.call("Example.call")
        assert isinstance(result, Rejected) and result.code == errno.EPROTO


def test_call_timeout_is_bounded(peer: PeerFactory) -> None:
    completed = threading.Event()

    def handle(connection: ServerConnection) -> None:
        _ = connection.recv()
        assert completed.wait(timeout=5)

    with accepted(Cdp.open(peer(handle))) as cdp:
        try:
            result = cdp.call("Example.call", timeout=0.02)
            assert isinstance(result, Rejected) and result.code == errno.ETIMEDOUT
        finally:
            completed.set()


def test_call_closed_peer_is_rejected(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        connection.close()

    with accepted(Cdp.open(peer(handle))) as cdp:
        result = cdp.call("Example.call")
        assert isinstance(result, Rejected) and result.code == errno.ECONNRESET


def test_open_invalid_websocket_uri_is_rejected() -> None:
    result = Cdp.open("not-a-websocket")
    assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_call_string_response_id_is_rejected(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        _ = connection.recv()
        connection.send('{"id":"1","result":{}}')

    with accepted(Cdp.open(peer(handle))) as cdp:
        assert isinstance(cdp.call("Example.call"), Rejected)


def test_call_reply_without_result_or_error_is_rejected(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        message = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(json.dumps({"id": message["id"]}))

    with accepted(Cdp.open(peer(handle))) as cdp:
        result = cdp.call("Example.call")
        assert isinstance(result, Rejected) and result.code == errno.EPROTO


class Answer(BaseModel):
    number: int


@pytest.mark.parametrize(
    "reply",
    [
        {"result": {"value": {"number": 3}}},
        {"result": {"value": {"wrong": 3}}},
        {"exceptionDetails": {"text": "JavaScript failed"}},
    ],
)
def test_evaluate_validates_value_and_rejects_script_or_schema_errors(
    peer: PeerFactory, reply: dict[str, JsonValue]
) -> None:
    def handle(connection: ServerConnection) -> None:
        message = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(json.dumps({"id": message["id"], "result": reply}))

    with accepted(Cdp.open(peer(handle))) as cdp:
        result = cdp.evaluate("({number:3})", Answer)
        if reply == {"result": {"value": {"number": 3}}}:
            assert accepted(result).number == 3
        else:
            assert isinstance(result, Rejected) and result.code == errno.EPROTO
