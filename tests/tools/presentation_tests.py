"""module: presentation_tests; responsibility: define observable frame timing.

operations: pytest cases; entities: none; tests: timing, unavailable, invalid CDP.
The peer implements only Chromium's public Runtime.evaluate wire contract.
"""

import errno
import json

import pytest
from pydantic import JsonValue, TypeAdapter
from websockets.sync.server import ServerConnection

from tests.conftest import PeerFactory
from tests.support.cdp import Cdp
from tests.support.outcome import Rejected, accepted
from tests.tools.presentation import Monitor


@pytest.mark.parametrize("callbacks", [0, 1, 3])
def test_read_distinguishes_missing_steady_intervals_from_zero_latency(
    peer: PeerFactory, callbacks: int
) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        assert request["method"] == "Runtime.evaluate"
        connection.send(
            json.dumps({"id": request["id"], "result": {"result": {"value": True}}})
        )
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps(
                {
                    "id": request["id"],
                    "result": {
                        "result": {
                            "value": {
                                "callbacks": callbacks,
                                "frames": callbacks,
                                "first_ms": 0.0,
                                "last_ms": 10.0,
                                "maximum_gap_ms": 33.0,
                                "media_time": 0.01,
                                "gaps": [{"elapsed": 0.01, "milliseconds": 33.0}]
                                if callbacks > 1
                                else [],
                            }
                        }
                    },
                }
            )
        )

    with accepted(Cdp.open(peer(handle))) as tab:
        monitor = accepted(Monitor.start(tab))
        timing = accepted(monitor.read(5))
        assert timing.callbacks == callbacks and timing.steady_gap is None
        assert timing.availability == (
            "no-callbacks" if callbacks == 0 else "no-steady-intervals"
        )


def test_read_preserves_raw_gaps_and_summarizes_after_warmup(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps({"id": request["id"], "result": {"result": {"value": True}}})
        )
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps(
                {
                    "id": request["id"],
                    "result": {
                        "result": {
                            "value": {
                                "callbacks": 5,
                                "frames": 5,
                                "first_ms": 0.0,
                                "last_ms": 7000.0,
                                "maximum_gap_ms": 1000.0,
                                "media_time": 7.0,
                                "gaps": [
                                    {"elapsed": 1.0, "milliseconds": 20.0},
                                    {"elapsed": 5.01, "milliseconds": 1000.0},
                                    {"elapsed": 6.0, "milliseconds": 30.0},
                                    {"elapsed": 7.0, "milliseconds": 50.0},
                                ],
                            }
                        }
                    },
                }
            )
        )

    with accepted(Cdp.open(peer(handle))) as tab:
        monitor = accepted(Monitor.start(tab))
        timing = accepted(monitor.read(5))
        assert timing.availability == "available" and timing.steady_gap is not None
        assert timing.steady_gap.mean == 40 and timing.steady_gap.maximum == 50
        assert len(timing.gaps) == 4 and timing.callbacks == 5
        assert timing.maximum_gap_ms == 1000


def test_start_remote_failure_is_rejected(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps(
                {
                    "id": request["id"],
                    "error": {"code": -32602, "message": "unavailable"},
                }
            )
        )

    with accepted(Cdp.open(peer(handle))) as tab:
        result = Monitor.start(tab)
        assert isinstance(result, Rejected) and result.code == -32602


@pytest.mark.parametrize("warmup", [-1.0, float("inf")])
def test_read_invalid_warmup_is_rejected_without_request(
    peer: PeerFactory, warmup: float
) -> None:
    def handle(connection: ServerConnection) -> None:
        request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
        connection.send(
            json.dumps({"id": request["id"], "result": {"result": {"value": True}}})
        )
        connection.close()

    with accepted(Cdp.open(peer(handle))) as tab:
        monitor = accepted(Monitor.start(tab))
        result = monitor.read(warmup)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_read_invalid_observation_is_rejected(peer: PeerFactory) -> None:
    def handle(connection: ServerConnection) -> None:
        for value in (True, {"callbacks": -1}):
            request = TypeAdapter(dict[str, JsonValue]).validate_json(connection.recv())
            connection.send(
                json.dumps(
                    {"id": request["id"], "result": {"result": {"value": value}}}
                )
            )

    with accepted(Cdp.open(peer(handle))) as tab:
        monitor = accepted(Monitor.start(tab))
        result = monitor.read(0)
        assert isinstance(result, Rejected) and result.code == errno.EPROTO
