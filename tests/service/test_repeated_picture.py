"""module: test_repeated_picture; responsibility: distinguish display and decode.

operations: pytest cases; entities: none; tests: repeated reference display,
timestamps, slot reuse and reference-preserving drain.
"""

import struct
from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec, load


@pytest.mark.parametrize("drain_at", [0, 1, 2])
def test_vp9_existing_picture_has_current_presentation_timestamp(
    mock_service: Process,
    artifacts: Path,
    tmp_path: Path,
    drain_at: int,
) -> None:
    # External failure reproduced with the rockchip_mpp 1.3.10 SDK:
    # this valid four-presentation stream
    # produces only the keyframe, both through the installed asynchronous
    # service and through the synchronous candidate. Direct Decoder tests
    # bypass the bridge and reproduce timestamps [0] rather than [0, 1, 2, 3].
    # vp9d_parser.c queues show_existing_frame in QUEUE_DISPLAY without a
    # hardware task; mpp_dec_no_thread.c does not publish that queue on its
    # non-EOS no-task path. That explains the synchronous reproduction only.
    # MPP remains unmodified by user decision. Keep the exact assertions;
    # this external blocker must not hide regressions in the remaining work.
    # Executable reproduction: crates/rkmpp/src/decoder_tests.rs,
    # test_packet_repeated_reference_publishes_each_presentation.
    source = artifacts / "vp9-yuv420p.ivf"
    key = accepted(load(Codec.VP9, source))[0].data
    # VP9 profile 0, show_existing_frame=1, frame_to_show_map_idx=0.
    payloads = (key, b"\x88", b"\x88", b"\x88")
    header = bytearray(source.read_bytes()[:32])
    struct.pack_into("<II", header, 16, 1, 1)
    struct.pack_into("<I", header, 24, len(payloads))
    stream = tmp_path / "repeated.ivf"
    _ = stream.write_bytes(
        bytes(header)
        + b"".join(
            struct.pack("<IQ", len(packet), index) + packet
            for index, packet in enumerate(payloads)
        )
    )
    expected = accepted(frame_hashes(stream, 8, 256, 128))
    assert len(expected) == 4 and len(set(expected)) == 1
    result = accepted(
        play(mock_service, Codec.VP9, stream, input_count=1, drain_at=drain_at)
    )
    assert result.eos and not result.errors
    assert tuple(frame.digest for frame in result.frames) == expected
    assert tuple(frame.timestamp for frame in result.frames) == (
        0,
        1_000_000_000,
        2_000_000_000,
        3_000_000_000,
    )
    assert len(result.input_completions) == len(payloads)
    assert all(done.flags == 0 for done in result.input_completions)
