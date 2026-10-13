"""module: window_tests; responsibility: specify background CDP target ownership.

operations: pytest cases; entities: none; tests:
    test_open_requests_background_window_and_close_releases_only_owned_target
    test_open_remote_error_is_rejected
    test_open_invalid_target_reply_is_rejected
    test_close_remote_error_preserves_target_for_retry
    test_close_invalid_reply_preserves_target_for_retry
    test_close_refused_target_preserves_target_for_retry
"""

import errno
import json

import pytest
from pydantic import JsonValue, TypeAdapter
from websockets.sync.server import ServerConnection

from tests.conftest import PeerFactory
from tests.support.cdp import Cdp
from tests.support.outcome import Rejected, accepted
from tests.support.window import Window


def test_open_requests_background_window_and_close_releases_only_owned_target(
    peer: PeerFactory,
) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        assert request["method"] == "Target.createTarget"
        assert request["params"] == {
            "url": "about:blank",
            "newWindow": True,
            "background": True,
        }
        connection.send(
            json.dumps({"id": request["id"], "result": {"targetId": "owned-page"}})
        )
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        assert request["method"] == "Target.closeTarget"
        assert request["params"] == {"targetId": "owned-page"}
        connection.send(json.dumps({"id": request["id"], "result": {"success": True}}))

    with accepted(Cdp.open(peer(handle))) as control:
        window = accepted(Window.open(control))
        assert window.target == "owned-page"
        assert accepted(window.close()) is None
        assert accepted(window.close()) is None


def test_open_remote_error_is_rejected(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps(
                {
                    "id": request["id"],
                    "error": {"code": -32602, "message": "cannot create window"},
                }
            )
        )

    with accepted(Cdp.open(peer(handle))) as control:
        result = Window.open(control)
        assert isinstance(result, Rejected) and result.code == -32602


@pytest.mark.parametrize("reply", [{}, {"targetId": ""}, {"targetId": 7}])
def test_open_invalid_target_reply_is_rejected(
    peer: PeerFactory, reply: dict[str, JsonValue]
) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(json.dumps({"id": request["id"], "result": reply}))

    with accepted(Cdp.open(peer(handle))) as control:
        result = Window.open(control)
        assert isinstance(result, Rejected) and result.code == errno.EPROTO


def test_close_remote_error_preserves_target_for_retry(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps({"id": request["id"], "result": {"targetId": "owned-page"}})
        )
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps(
                {
                    "id": request["id"],
                    "error": {"code": -32000, "message": "retry close"},
                }
            )
        )
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        assert request["params"] == {"targetId": "owned-page"}
        connection.send(json.dumps({"id": request["id"], "result": {"success": True}}))

    with accepted(Cdp.open(peer(handle))) as control:
        window = accepted(Window.open(control))
        result = window.close()
        assert isinstance(result, Rejected) and result.code == -32000
        assert accepted(window.close()) is None


@pytest.mark.parametrize("reply", [{}, {"success": "true"}])
def test_close_invalid_reply_preserves_target_for_retry(
    peer: PeerFactory, reply: dict[str, JsonValue]
) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps({"id": request["id"], "result": {"targetId": "owned-page"}})
        )
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(json.dumps({"id": request["id"], "result": reply}))
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        assert request["params"] == {"targetId": "owned-page"}
        connection.send(json.dumps({"id": request["id"], "result": {"success": True}}))

    with accepted(Cdp.open(peer(handle))) as control:
        window = accepted(Window.open(control))
        result = window.close()
        assert isinstance(result, Rejected) and result.code == errno.EPROTO
        assert accepted(window.close()) is None


def test_close_refused_target_preserves_target_for_retry(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps({"id": request["id"], "result": {"targetId": "owned-page"}})
        )
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(json.dumps({"id": request["id"], "result": {"success": False}}))
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        assert request["params"] == {"targetId": "owned-page"}
        connection.send(json.dumps({"id": request["id"], "result": {"success": True}}))

    with accepted(Cdp.open(peer(handle))) as control:
        window = accepted(Window.open(control))
        result = window.close()
        assert isinstance(result, Rejected) and result.code == errno.EIO
        assert accepted(window.close()) is None
