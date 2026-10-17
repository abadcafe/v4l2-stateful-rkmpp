"""module: test_concurrency; responsibility: specify independent process workers.

operations: pytest cases; entities: none; tests: same/mixed codecs preserve pixels.
"""

import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import accepted
from tests.support.reference import frame_hashes
from tests.support.stream import Codec


@pytest.mark.parametrize(
    "codecs", [(Codec.VP9, Codec.VP9), (Codec.VP9, Codec.AV1)], ids=["same", "mixed"]
)
def test_decode_concurrent_workers_preserve_all_registered_buffers(
    mock_service: Process, artifacts: Path, codecs: tuple[Codec, Codec]
) -> None:
    inputs = {Codec.VP9: "vp9-yuv420p10le.ivf", Codec.AV1: "av1-yuv420p10le.ivf"}
    expected = {
        codec: accepted(frame_hashes(artifacts / inputs[codec], 10, 256, 128))
        for codec in codecs
    }
    ready = threading.Barrier(2)

    def run(codec: Codec) -> tuple[str, ...]:
        _ = ready.wait(timeout=5)
        decoded = accepted(
            play(
                mock_service,
                codec,
                artifacts / inputs[codec],
            )
        )
        assert decoded.eos and len(decoded.frames) == 36
        assert all(frame.flags == 0 for frame in decoded.frames), [
            frame.flags for frame in decoded.frames
        ]
        return tuple(frame.digest for frame in decoded.frames)

    with ThreadPoolExecutor(max_workers=2) as executor:
        for _ in range(64):
            futures = [executor.submit(run, codec) for codec in codecs]
            for codec, future in zip(codecs, futures, strict=True):
                assert future.result(timeout=30) == expected[codec]
