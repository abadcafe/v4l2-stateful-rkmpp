"""module: mock_decode_tests; responsibility: specify decode transaction boundaries.

operations: pytest cases; entities: none; tests: missing inputs/invalid repetitions.
"""

import errno
from pathlib import Path

import pytest

from tests.support.mock_decode import play
from tests.support.mock_process import Process
from tests.support.outcome import Rejected, accepted
from tests.support.stream import Codec, Packet


@pytest.mark.parametrize("timestamp", [-1, (1 << 64) // 1000 + 1, 1 << 128])
@pytest.mark.parametrize("override", [False, True])
def test_play_unrepresentable_timestamp_rejects_before_session_open(
    executable: Path, tmp_path: Path, timestamp: int, override: bool
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        result = play(
            process,
            Codec.H264,
            tmp_path / "unused",
            submissions=(
                Packet(data=b"input", timestamp=0 if override else timestamp),
            ),
            timestamps=(timestamp,) if override else (),
        )
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
        assert "session stats" not in process.log()


@pytest.mark.parametrize("repetitions", [0, -1, 33])
def test_play_invalid_repetition_count_rejects_before_session_open(
    executable: Path, tmp_path: Path, repetitions: int
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        result = play(process, Codec.H264, tmp_path / "missing", repetitions)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
        assert "session stats" not in process.log()


def test_play_missing_input_returns_filesystem_error(
    executable: Path,
    tmp_path: Path,
) -> None:
    library = executable.parent.parent / "debug/libv4l2_rkmpp_kernel_mock.so"
    with accepted(Process.open(executable, library, tmp_path / "mock")) as process:
        result = play(process, Codec.H264, tmp_path / "missing")
        assert isinstance(result, Rejected) and result.code == errno.ENOENT
        assert "session stats" not in process.log()
