"""module: tests.support.event_abi_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_control_wire_sizes_match_linux_aarch64_abi
    test_source_change_event_and_decoder_command_preserve_fields


"""

import ctypes

import pytest

from tests.support.event_abi import (
    DecoderCommand,
    Event,
    Scalar,
    Subscription,
    Timespec,
)
from tests.support.outcome import accepted
from tests.support.uapi import verify


def test_control_wire_sizes_match_linux_aarch64_abi() -> None:
    assert [
        ctypes.sizeof(t) for t in (Event, Subscription, DecoderCommand, Scalar)
    ] == [136, 32, 72, 4]


def test_source_change_event_and_decoder_command_preserve_fields() -> None:
    event = Event(kind=5)
    event.payload.changes = 1
    copy = Event.from_buffer_copy(bytes(event))
    assert copy.kind == 5 and copy.payload.changes == 1
    command = DecoderCommand(command=1)
    assert DecoderCommand.from_buffer_copy(bytes(command)).command == 1


@pytest.mark.parametrize(
    ("wire_type", "c_type", "fields"),
    [
        (
            Event,
            "v4l2_event",
            (
                ("kind", "type"),
                ("payload", "u"),
                ("pending", "pending"),
                ("sequence", "sequence"),
                ("timestamp", "timestamp"),
                ("id", "id"),
                ("reserved", "reserved"),
            ),
        ),
        (
            Subscription,
            "v4l2_event_subscription",
            (
                ("kind", "type"),
                ("id", "id"),
                ("flags", "flags"),
                ("reserved", "reserved"),
            ),
        ),
        (
            DecoderCommand,
            "v4l2_decoder_cmd",
            (("command", "cmd"), ("data", "raw"), ("flags", "flags")),
        ),
        (Timespec, "timespec", (("seconds", "tv_sec"), ("nanoseconds", "tv_nsec"))),
    ],
)
def test_wire_all_fields_match_the_compiled_public_c_header(
    wire_type: type[ctypes.Structure],
    c_type: str,
    fields: tuple[tuple[str, str], ...],
) -> None:
    assert accepted(verify(wire_type, c_type, fields)) is None
