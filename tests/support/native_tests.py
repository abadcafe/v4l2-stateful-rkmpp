"""module: native_tests; responsibility: define the native test ABI contract.

operations: pytest cases; entities: none;
tests: layout, zero initialization, exact decoding, snapshots, bounds, raw inputs.
"""

import ast
import errno
import struct
import subprocess
from importlib.resources import files
from pathlib import Path
from types import GetSetDescriptorType

import pytest
from pydantic import TypeAdapter
from rkmpp_bridge_uapi import (
    EXCHANGE,
    REGISTER_POOL,
    VERSION,
    Batch,
    Descriptor,
    Event,
    Message,
    Pool,
    Reply,
)

ROOT = Path(__file__).resolve().parents[2]


def test_native_packaged_stub_covers_all_fields_and_enum_members() -> None:
    stub = ast.parse(files("rkmpp_bridge_uapi").joinpath("__init__.pyi").read_text())
    classes = {node.name: node for node in stub.body if isinstance(node, ast.ClassDef)}
    for record in (Message, Batch, Descriptor, Pool):
        schema = classes[record.__name__]
        fields = {
            node.target.id
            for node in schema.body
            if isinstance(node, ast.AnnAssign)
            and isinstance(node.target, ast.Name)
            and not (
                isinstance(node.annotation, ast.Subscript)
                and isinstance(node.annotation.value, ast.Name)
                and node.annotation.value.id == "ClassVar"
            )
        }
        members = TypeAdapter(dict[str, object]).validate_python(vars(record))
        assert fields == {
            name
            for name, value in members.items()
            if isinstance(value, GetSetDescriptorType) and not name.startswith("_")
        }
        constructor = next(
            node
            for node in schema.body
            if isinstance(node, ast.FunctionDef) and node.name == "__init__"
        )
        assert fields == {argument.arg for argument in constructor.args.kwonlyargs}
    for enumeration in (Event, Reply):
        assert set(enumeration.__members__) == {
            target.id
            for node in classes[enumeration.__name__].body
            if isinstance(node, ast.Assign)
            for target in node.targets
            if isinstance(target, ast.Name)
        }


def test_native_layout_matches_the_wire_contract() -> None:
    assert [(t.SIZE, t.ALIGN) for t in (Message, Batch, Descriptor, Pool)] == [
        (120, 8),
        (3920, 8),
        (8, 4),
        (288, 8),
    ]
    assert EXCHANGE == 0xCF505284


def test_native_defaults_zero_every_wire_byte() -> None:
    for record in (Message(), Batch(), Pool(), Descriptor()):
        assert bytes(record) == bytes(record.SIZE)


@pytest.mark.parametrize("length", [0, 119, 121])
def test_native_decode_rejects_inexact_message_length(length: int) -> None:
    assert Message.decode(bytes(length)) is None


def test_native_message_roundtrip_preserves_invalid_protocol_values() -> None:
    message = Message(version=0xFFFFFFFF, kind=0xFFFFFFFF, reserved=7, epoch=2**64 - 1)
    decoded = Message.decode(bytes(message))
    assert decoded is not None
    assert (decoded.version, decoded.kind, decoded.reserved, decoded.epoch) == (
        0xFFFFFFFF,
        0xFFFFFFFF,
        7,
        2**64 - 1,
    )
    assert struct.unpack_from("=I", bytes(message))[0] == 0xFFFFFFFF


def test_native_batch_elements_are_independent_snapshots() -> None:
    batch = Batch(version=VERSION)
    message = Message(kind=Event.PACKET, index=9)
    assert batch.set_event(0, message)
    event = batch.event(0)
    assert event is not None
    event.index = 10
    current = batch.event(0)
    assert current is not None and current.index == 9
    assert batch.set_event(0, event)
    current = batch.event(0)
    assert current is not None and current.index == 10
    assert message.index == 9


def test_native_array_bounds_return_optional_or_false() -> None:
    batch = Batch()
    assert batch.event(16) is None and batch.reply(16) is None
    assert not batch.set_event(16, Message())
    assert not batch.set_reply(16, Message())
    assert not batch.set_result(16, 0)
    pool = Pool()
    assert pool.descriptor(32) is None
    assert not pool.set_descriptor(32, Descriptor())


@pytest.mark.parametrize(
    "index",
    [-1, -(1 << 256), 1 << 32, 1 << 64, 1 << 256],
    ids=[
        "negative",
        "negative-wide",
        "large-native",
        "usize-overflow",
        "wide-positive",
    ],
)
def test_native_array_invalid_integer_returns_without_mutating_records(
    index: int,
) -> None:
    batch = Batch(version=VERSION)
    assert batch.set_event(0, Message(index=7))
    assert batch.set_reply(0, Message(index=9))
    assert batch.set_result(0, -errno.ESTALE)
    pool = Pool(version=VERSION)
    assert pool.set_descriptor(0, Descriptor(fd=-17, size=4096))
    before = (bytes(batch), bytes(pool))
    assert batch.event(index) is None
    assert batch.reply(index) is None
    assert batch.result(index) is None
    assert batch.results(index) is None
    assert not batch.set_event(index, Message())
    assert not batch.set_reply(index, Message())
    assert not batch.set_result(index, 0)
    assert pool.descriptor(index) is None
    assert not pool.set_descriptor(index, Descriptor())
    assert Pool.descriptor_offset(index) is None
    assert (bytes(batch), bytes(pool)) == before
    assert batch.results(1) == [-errno.ESTALE]


def test_native_results_read_only_the_requested_entries() -> None:
    batch = Batch()
    assert batch.set_result(0, -errno.ESTALE)
    assert batch.result(0) == -errno.ESTALE
    assert batch.result(16) is None
    assert batch.results(2) == [-errno.ESTALE, 0]
    assert batch.results(17) is None


def test_native_ioctl_reports_errno_without_raising() -> None:
    assert Batch().ioctl(-1, EXCHANGE) == -errno.EBADF
    assert Batch().ioctl_readonly(-1, EXCHANGE) == -errno.EBADF


def test_native_ioctl_rejects_command_with_a_different_storage_contract() -> None:
    assert Batch().ioctl(-1, REGISTER_POOL) == -errno.EINVAL
    assert Pool().ioctl_readonly(-1, EXCHANGE) == -errno.EINVAL


def test_native_pool_owns_descriptors_and_preserves_signed_fd_values() -> None:
    pool = Pool()
    descriptor = Descriptor(fd=-17, size=4096)
    assert pool.set_descriptor(31, descriptor)
    descriptor.fd = 42
    decoded = Pool.decode(bytes(pool))
    assert decoded is not None
    current = decoded.descriptor(31)
    assert current is not None and (current.fd, current.size) == (-17, 4096)
    assert Pool.descriptor_offset(31) == 32 + 31 * 8
    assert Pool.descriptor_offset(32) is None
    assert Pool.decode(bytes(Pool.SIZE - 1)) is None
    assert Batch.decode(bytes(Batch.SIZE + 1)) is None


def test_native_enums_and_flags_use_the_kernel_discriminants() -> None:
    assert [int(value) for value in Event] == list(range(1, 8))
    assert [int(value) for value in Reply] == list(range(32, 39))
    assert Message.CAPTURE_REUSE == 1 and Message.FRAME_ERROR == 2
    assert Message.RESET_CAPTURE_ONLY == 1 << 16
    assert Message.RESET_ABORT_DRAIN == 1 << 17


def test_native_message_matches_bytes_written_by_an_independent_c_client(
    tmp_path: Path,
) -> None:
    fields = (
        "version",
        "kind",
        "epoch",
        "index",
        "flags",
        "timestamp",
        "bytesused",
        "codec",
        "width",
        "height",
        "stride",
        "rows",
        "size",
        "format",
        "count",
        "generation",
        "colorspace",
        "xfer_func",
        "ycbcr_enc",
        "quantization",
        "pool_id",
        "data_offset",
        "crop_left",
        "crop_top",
        "coded_width",
        "coded_height",
        "field",
        "reserved",
    )
    values = {field: index + 100 for index, field in enumerate(fields)}
    values["epoch"], values["timestamp"] = 2**63 + 7, 2**64 - 1
    names = {"kind": "type", "reserved": "reserved[0]"}
    assignments = [
        f"message.{names.get(name, name)} = {value}ULL;"
        for name, value in values.items()
    ]
    source = "\n".join(
        [
            "#include <stdio.h>",
            '#include "bridge_uapi.h"',
            "int main(void) { struct rb_message message = {0};",
            *assignments,
            "return fwrite(&message, sizeof(message), 1, stdout) == 1 ? 0 : 1; }",
        ]
    )
    executable = tmp_path / "wire-probe"
    built = subprocess.run(
        [
            "cc",
            "-x",
            "c",
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-I",
            str(ROOT / "kernel"),
            "-o",
            str(executable),
            "-",
        ],
        input=source,
        capture_output=True,
        text=True,
        check=False,
        timeout=15,
    )
    assert built.returncode == 0, built.stderr
    written = subprocess.run(
        [str(executable)],
        capture_output=True,
        check=False,
        timeout=5,
    )
    assert written.returncode == 0, written.stderr
    message = Message.decode(written.stdout)
    assert message is not None
    assert bytes(Message(**values)) == written.stdout
    assert bytes(message.clone()) == written.stdout
