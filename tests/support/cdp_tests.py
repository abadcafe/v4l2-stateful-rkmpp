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
    "timeout", [0.0, -1.0, float("nan"), float("inf"), -float("inf")]
)
def test_call_invalid_deadline_rejects_before_sending(
    peer: PeerFactory, timeout: float
) -> None:
    def handle(connection: ServerConnection) -> None:
        message = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        assert message["method"] == "Example.valid" and message["id"] == 1
        connection.send(json.dumps({"id": 1, "result": {}}))

    with accepted(Cdp.open(peer(handle))) as cdp:
        result = cdp.call("Example.invalid", timeout=timeout)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
        assert accepted(cdp.call("Example.valid")) == {}


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


def test_call_after_timeout_discards_late_reply_and_preserves_notifications(
    peer: PeerFactory,
) -> None:
    def handle(connection: ServerConnection) -> None:
        first = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        second = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(json.dumps({"id": first["id"], "result": {"late": True}}))
        connection.send('{"method":"Example.changed","params":{}}')
        connection.send(json.dumps({"id": second["id"], "result": {"current": True}}))

    with accepted(Cdp.open(peer(handle))) as cdp:
        result = cdp.call("Example.slow", timeout=0.02)
        assert isinstance(result, Rejected) and result.code == errno.ETIMEDOUT
        assert accepted(cdp.call("Example.next")) == {"current": True}
        assert [event.method for event in cdp.events()] == ["Example.changed"]


def test_call_rejects_duplicate_reply_from_completed_command(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        first = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        response = json.dumps({"id": first["id"], "result": {}})
        connection.send(response)
        _ = connection.recv()
        connection.send(response)

    with accepted(Cdp.open(peer(handle))) as cdp:
        assert accepted(cdp.call("Example.first")) == {}
        result = cdp.call("Example.second")
        assert isinstance(result, Rejected) and result.code == errno.EPROTO


def test_call_expired_deadline_does_not_consume_buffered_messages(
    peer: PeerFactory,
) -> None:
    buffered = threading.Event()
    completed = threading.Event()

    def handle(connection: ServerConnection) -> None:
        connection.send('{"method":"Example.changed","params":{}}')
        # The pong proves that the client has received the preceding frame.
        assert connection.ping().wait(timeout=5)
        buffered.set()
        _ = connection.recv()
        assert completed.wait(timeout=5)

    with accepted(Cdp.open(peer(handle))) as cdp:
        try:
            assert buffered.wait(timeout=5)
            result = cdp.call("Example.call", timeout=1e-300)
            assert isinstance(result, Rejected) and result.code == errno.ETIMEDOUT
            assert cdp.events() == ()
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
