"""module: controls_tests; responsibility: define reversible measurement controls.

operations: pytest cases; entities: none; tests: change, restore, rollback, access.
Temporary files substitute the public sysfs/procfs interface; hardware is untouched.
"""

import errno
import sys
from pathlib import Path

import pytest

from tests.support.outcome import Rejected, accepted
from tests.tools.performance.controls import Controls, Paths


@pytest.fixture
def paths(tmp_path: Path) -> Paths:
    files = Paths(
        minimum=tmp_path / "min",
        maximum=tmp_path / "max",
        interval=tmp_path / "interval",
    )
    _ = files.minimum.write_text("200000000\n")
    _ = files.maximum.write_text("1000000000\n")
    _ = files.interval.write_text("0\n")
    return files


def test_open_normal_gpu_policy_only_changes_sampling_period(paths: Paths) -> None:
    control = accepted(Controls.open(None, 1000, paths))
    assert paths.minimum.read_text() == "200000000\n"
    assert paths.interval.read_text() == "1000"
    assert control.settings.original_minimum_hz == 200000000
    assert control.settings.requested_minimum_hz is None
    assert accepted(control.close()) is None
    assert paths.interval.read_text() == "0\n"


@pytest.mark.parametrize("frequency", [None, 1000000000])
def test_open_unrenderable_interval_rejects_before_any_control_write(
    paths: Paths, monkeypatch: pytest.MonkeyPatch, frequency: int | None
) -> None:
    original_limit = sys.get_int_max_str_digits()
    writes: list[Path] = []
    write = Path.write_text

    def record(
        path: Path,
        data: str,
        encoding: str | None = None,
        errors: str | None = None,
        newline: str | None = None,
    ) -> int:
        writes.append(path)
        return write(path, data, encoding=encoding, errors=errors, newline=newline)

    monkeypatch.setattr(Path, "write_text", record)
    try:
        sys.set_int_max_str_digits(sys.int_info.str_digits_check_threshold)
        interval = 1 << (4 * sys.int_info.str_digits_check_threshold)
        result = Controls.open(frequency, interval, paths)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
        assert not writes
        assert paths.minimum.read_text() == "200000000\n"
        assert paths.interval.read_text() == "0\n"
    finally:
        sys.set_int_max_str_digits(original_limit)
    control = accepted(Controls.open(frequency, 1000, paths))
    assert accepted(control.close()) is None


def test_open_fixed_frequency_and_close_restore_exact_values(paths: Paths) -> None:
    control = accepted(Controls.open(1000000000, 500, paths))
    assert (
        paths.minimum.read_text() == "1000000000"
        and paths.interval.read_text() == "500"
    )
    assert accepted(control.close()) is None
    assert (
        paths.minimum.read_text() == "200000000\n"
        and paths.interval.read_text() == "0\n"
    )
    assert accepted(control.close()) is None


@pytest.mark.parametrize(
    "frequency,interval", [(0, 1000), (1000000001, 1000), (None, 0)]
)
def test_open_invalid_control_settings_do_not_change_files(
    paths: Paths, frequency: int | None, interval: int
) -> None:
    assert isinstance(Controls.open(frequency, interval, paths), Rejected)
    assert (
        paths.minimum.read_text() == "200000000\n"
        and paths.interval.read_text() == "0\n"
    )


def test_open_sampling_write_failure_rolls_back_frequency(
    paths: Paths, monkeypatch: pytest.MonkeyPatch
) -> None:
    write = Path.write_text

    def denied(
        path: Path,
        data: str,
        encoding: str | None = None,
        errors: str | None = None,
        newline: str | None = None,
    ) -> int:
        if path == paths.interval:
            raise PermissionError(errno.EACCES, "video group permission missing")
        return write(path, data, encoding=encoding, errors=errors, newline=newline)

    monkeypatch.setattr(Path, "write_text", denied)
    result = Controls.open(1000000000, 1000, paths)
    assert isinstance(result, Rejected) and result.code == errno.EACCES
    assert paths.minimum.read_text() == "200000000\n"


def test_close_reports_failed_restoration_and_can_retry(
    paths: Paths, monkeypatch: pytest.MonkeyPatch
) -> None:
    control = accepted(Controls.open(1000000000, 1000, paths))
    write = Path.write_text

    def denied(
        path: Path,
        data: str,
        encoding: str | None = None,
        errors: str | None = None,
        newline: str | None = None,
    ) -> int:
        if path == paths.minimum:
            raise PermissionError(errno.EACCES, "restoration denied")
        return write(path, data, encoding=encoding, errors=errors, newline=newline)

    monkeypatch.setattr(Path, "write_text", denied)
    result = control.close()
    assert isinstance(result, Rejected)
    assert paths.interval.read_text() == "0\n"
    monkeypatch.setattr(Path, "write_text", write)
    assert accepted(control.close()) is None


@pytest.mark.parametrize("stage", ["minimum", "interval"])
def test_open_interruption_after_write_restores_both_original_values(
    paths: Paths, monkeypatch: pytest.MonkeyPatch, stage: str
) -> None:
    write = Path.write_text
    interrupted = paths.minimum if stage == "minimum" else paths.interval
    requested = "1000000000" if stage == "minimum" else "1000"

    def interrupt(
        path: Path,
        data: str,
        encoding: str | None = None,
        errors: str | None = None,
        newline: str | None = None,
    ) -> int:
        written = write(path, data, encoding=encoding, errors=errors, newline=newline)
        if path == interrupted and data == requested:
            raise KeyboardInterrupt
        return written

    monkeypatch.setattr(Path, "write_text", interrupt)
    result = Controls.open(1000000000, 1000, paths)
    assert isinstance(result, Rejected) and result.code == errno.ECANCELED
    assert paths.minimum.read_text() == "200000000\n"
    assert paths.interval.read_text() == "0\n"


def test_open_failed_write_after_partial_update_restores_original(
    paths: Paths, monkeypatch: pytest.MonkeyPatch
) -> None:
    write = Path.write_text

    def failed(
        path: Path,
        data: str,
        encoding: str | None = None,
        errors: str | None = None,
        newline: str | None = None,
    ) -> int:
        written = write(path, data, encoding=encoding, errors=errors, newline=newline)
        if path == paths.interval and data == "1000":
            raise OSError(errno.EIO, "write completion failed")
        return written

    monkeypatch.setattr(Path, "write_text", failed)
    result = Controls.open(1000000000, 1000, paths)
    assert isinstance(result, Rejected) and result.code == errno.EIO
    assert paths.minimum.read_text() == "200000000\n"
    assert paths.interval.read_text() == "0\n"
