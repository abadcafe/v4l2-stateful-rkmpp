"""module: tests.support.graphics_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_get_info_exposes_renderer_and_compositor_backend
    test_get_info_missing_or_invalid_gpu_attributes_are_rejected
    test_get_info_remote_failure_is_propagated

Browser GPU information is validated at the public CDP boundary.
"""

import json

import pytest
from pydantic import JsonValue, TypeAdapter
from websockets.sync.server import ServerConnection

from tests.conftest import PeerFactory
from tests.support.cdp import Cdp
from tests.support.graphics import get_info
from tests.support.outcome import Rejected, accepted


def test_get_info_exposes_renderer_and_compositor_backend(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        assert request["method"] == "SystemInfo.getInfo"
        connection.send(
            json.dumps(
                {
                    "id": request["id"],
                    "result": {
                        "gpu": {
                            "auxAttributes": {
                                "skiaBackendType": "GaneshVulkan",
                                "glRenderer": "Mali-G610",
                            }
                        }
                    },
                }
            )
        )

    with accepted(Cdp.open(peer(handle))) as cdp:
        info = accepted(get_info(cdp))
        assert (info.skia, info.renderer) == ("GaneshVulkan", "Mali-G610")


@pytest.mark.parametrize(
    "payload",
    [
        {},
        {"gpu": {}},
        {"gpu": {"auxAttributes": {"skiaBackendType": 3, "glRenderer": "Mali"}}},
    ],
)
def test_get_info_missing_or_invalid_gpu_attributes_are_rejected(
    peer: PeerFactory, payload: dict[str, JsonValue]
) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(json.dumps({"id": request["id"], "result": payload}))

    with accepted(Cdp.open(peer(handle))) as cdp:
        assert isinstance(get_info(cdp), Rejected)


def test_get_info_remote_failure_is_propagated(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps(
                {"id": request["id"], "error": {"code": -32601, "message": "missing"}}
            )
        )

    with accepted(Cdp.open(peer(handle))) as cdp:
        result = get_info(cdp)
        assert isinstance(result, Rejected) and result.code == -32601
