"""module: tests.support.player_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_media_properties_keeps_latest_property_value
    test_media_properties_malformed_notification_is_rejected

State parsing uses public CDP notifications and DOM values.
"""

import errno
import json
from typing import Literal

import pytest
from pydantic import JsonValue, TypeAdapter
from websockets.sync.server import ServerConnection

from tests.conftest import PeerFactory
from tests.support.cdp import Cdp, Notification
from tests.support.outcome import Rejected, accepted
from tests.support.player import Player, media_properties


def test_media_properties_keeps_latest_property_value() -> None:
    events = (
        Notification(method="Other.event", params={}),
        Notification(
            method="Media.playerPropertiesChanged",
            params={
                "playerId": "1",
                "properties": [
                    {"name": "kVideoDecoderName", "value": "FFmpegVideoDecoder"}
                ],
            },
        ),
        Notification(
            method="Media.playerPropertiesChanged",
            params={
                "playerId": "1",
                "properties": [
                    {"name": "kVideoDecoderName", "value": "V4L2VideoDecoder"},
                    {"name": "kIsPlatformVideoDecoder", "value": "true"},
                ],
            },
        ),
    )
    assert accepted(media_properties(events)) == {
        "kVideoDecoderName": "V4L2VideoDecoder",
        "kIsPlatformVideoDecoder": "true",
    }


def test_media_properties_malformed_notification_is_rejected() -> None:
    result = media_properties(
        (
            Notification(
                method="Media.playerPropertiesChanged",
                params={"properties": [{"name": 3}]},
            ),
        )
    )
    assert isinstance(result, Rejected)


@pytest.mark.parametrize("operation", ["load", "replay", "pause"])
def test_playback_operation_returns_browser_result(
    peer: PeerFactory, operation: Literal["load", "replay", "pause"]
) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        assert request["method"] == "Runtime.evaluate"
        connection.send(
            json.dumps({"id": request["id"], "result": {"result": {"value": True}}})
        )

    with accepted(Cdp.open(peer(handle))) as tab:
        player = Player(tab)
        result = (
            player.load("http://127.0.0.1/video.webm")
            if operation == "load"
            else (player.replay() if operation == "replay" else player.pause())
        )
        assert accepted(result)


@pytest.mark.parametrize("operation", ["load", "replay", "pause", "state"])
def test_playback_operation_propagates_browser_failure(
    peer: PeerFactory, operation: Literal["load", "replay", "pause", "state"]
) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps(
                {
                    "id": request["id"],
                    "error": {"code": -32000, "message": "player destroyed"},
                }
            )
        )

    with accepted(Cdp.open(peer(handle))) as tab:
        player = Player(tab)
        match operation:
            case "load":
                result = player.load("http://127.0.0.1/video.webm")
            case "replay":
                result = player.replay()
            case "pause":
                result = player.pause()
            case "state":
                result = player.state()
        assert isinstance(result, Rejected) and result.code == -32000


@pytest.mark.parametrize("valid", [True, False])
def test_state_accepts_complete_playback_and_rejects_malformed_values(
    peer: PeerFactory, valid: bool
) -> None:
    value: dict[str, JsonValue] = (
        {
            "time": 1.0,
            "ended": False,
            "paused": False,
            "error": None,
            "width": 256,
            "height": 128,
            "frames": {"total": 10, "dropped": 1},
            "hdr": False,
            "visible": True,
            "focused": False,
            "box": {"x": 0.0, "y": 0.0, "width": 768.0, "height": 384.0},
        }
        if valid
        else {"frames": "invalid"}
    )

    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps({"id": request["id"], "result": {"result": {"value": value}}})
        )

    with accepted(Cdp.open(peer(handle))) as tab:
        result = Player(tab).state()
        if valid:
            playback = accepted(result)
            assert playback.time == 1 and playback.frames.dropped == 1
            assert playback.visible and not playback.focused
        else:
            assert isinstance(result, Rejected) and result.code == errno.EPROTO
