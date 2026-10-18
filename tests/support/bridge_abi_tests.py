"""module: tests.support.bridge_abi_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_wire_layout_and_command_sizes_match_published_contract
    test_wire_defaults_zero_all_reserved_fields
    test_wire_defaults_isolate_each_message
    test_event_and_reply_discriminants_match_kernel_uapi


"""

import ctypes

import pytest

from tests.support.bridge_abi import (
    ACCEPT,
    EXCHANGE,
    MAX_BATCH,
    MAX_BUFFERS,
    REGISTER_POOL,
    VERSION,
    Batch,
    Descriptor,
    Event,
    Message,
    Pool,
    Reply,
)
from tests.support.outcome import accepted
from tests.support.uapi import verify


def test_wire_layout_and_command_sizes_match_published_contract() -> None:
    assert [
        (ctypes.sizeof(t), ctypes.alignment(t))
        for t in (Message, Batch, Descriptor, Pool)
    ] == [(120, 8), (3920, 8), (8, 4), (288, 8)]
    assert ACCEPT == 0x5280 and EXCHANGE == 0xCF505284 and REGISTER_POOL == 0xC1205285


def test_wire_defaults_zero_all_reserved_fields() -> None:
    assert bytes(Message()) == bytes(120)
    assert bytes(Batch()) == bytes(3920)
    assert bytes(Pool()) == bytes(288)
    assert VERSION == 9


def test_wire_defaults_isolate_each_message() -> None:
    first, second = Message(), Message()
    first.reserved[0] = 7
    first.index = 12
    assert bytes(second) == bytes(120)
    assert first.index == 12 and first.reserved[0] == 7


def test_event_and_reply_discriminants_match_kernel_uapi() -> None:
    assert list(Event) == list(range(1, 8))
    assert list(Reply) == list(range(32, 39))


@pytest.mark.parametrize(
    ("wire_type", "c_type", "fields"),
    [
        (
            Message,
            "rb_message",
            (
                ("kind", "type"),
                ("version", "version"),
                ("epoch", "epoch"),
                ("index", "index"),
                ("flags", "flags"),
                ("timestamp", "timestamp"),
                ("bytesused", "bytesused"),
                ("codec", "codec"),
                ("width", "width"),
                ("height", "height"),
                ("stride", "stride"),
                ("rows", "rows"),
                ("size", "size"),
                ("format", "format"),
                ("count", "count"),
                ("generation", "generation"),
                ("colorspace", "colorspace"),
                ("xfer_func", "xfer_func"),
                ("ycbcr_enc", "ycbcr_enc"),
                ("quantization", "quantization"),
                ("pool_id", "pool_id"),
                ("data_offset", "data_offset"),
                ("crop_left", "crop_left"),
                ("crop_top", "crop_top"),
                ("coded_width", "coded_width"),
                ("coded_height", "coded_height"),
                ("field", "field"),
                ("reserved", "reserved"),
            ),
        ),
        (
            Batch,
            "rb_batch",
            (
                ("version", "version"),
                ("reply_count", "reply_count"),
                ("event_count", "event_count"),
                ("reserved", "reserved"),
                ("replies", "replies"),
                ("events", "events"),
                ("results", "results"),
            ),
        ),
        (Descriptor, "rb_registered_buffer", (("fd", "fd"), ("size", "size"))),
        (
            Pool,
            "rb_pool",
            (
                ("kind", "type"),
                ("version", "version"),
                ("epoch", "epoch"),
                ("pool_id", "pool_id"),
                ("count", "count"),
                ("reserved", "reserved"),
                ("buffers", "buffers"),
            ),
        ),
    ],
)
def test_wire_all_fields_match_the_compiled_public_c_header(
    wire_type: type[ctypes.Structure],
    c_type: str,
    fields: tuple[tuple[str, str], ...],
) -> None:
    assert accepted(verify(wire_type, c_type, fields)) is None


def test_wire_constants_and_commands_match_the_compiled_kernel_header() -> None:
    constants = (
        ("RB_VERSION", VERSION),
        ("RB_BATCH_MAX", MAX_BATCH),
        ("RB_MAX_BUFFERS", MAX_BUFFERS),
        ("RB_ACCEPT", ACCEPT),
        ("RB_EXCHANGE", EXCHANGE),
        ("RB_REGISTER_POOL", REGISTER_POOL),
    )
    constants += tuple(("RB_" + event.name, int(event)) for event in Event)
    constants += tuple(("RB_" + response.name, int(response)) for response in Reply)
    assert accepted(verify(Message, "rb_message", (), constants)) is None
