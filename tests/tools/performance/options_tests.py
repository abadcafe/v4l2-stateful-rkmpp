"""module: options_tests; responsibility: define measurement command inputs.

operations: pytest cases; entities: none; tests: presets, custom input, validation.
Only public parsing and filesystem interfaces are used.
"""

import errno
from pathlib import Path

import pytest

from tests.support.outcome import Rejected, accepted
from tests.tools.performance.options import parse


@pytest.fixture
def source(tmp_path: Path) -> Path:
    directory = tmp_path / "tests/artifacts"
    directory.mkdir(parents=True)
    for name in (
        "h264-4k30-seed.mp4",
        "vp9-10-4k30-seed.webm",
        "av1-10-4k30-seed.webm",
    ):
        _ = (directory / name).write_bytes(b"video")
    return tmp_path


def test_parse_defaults_select_three_presets_and_normal_gpu_policy(
    source: Path,
) -> None:
    options = accepted(parse((), source))
    assert [workload.name for workload in options.workloads] == ["h264", "vp9", "av1"]
    assert all(workload.expand_frames == 900 for workload in options.workloads)
    assert options.rounds == 1 and options.gpu_minimum_hz is None
    assert options.interval_ms == 1000 and options.warmup_seconds == 5
    assert options.output.parent == source / "target/benchmarks"


def test_parse_explicit_inputs_preserve_selection_and_measurement_settings(
    source: Path,
) -> None:
    options = accepted(
        parse(
            (
                "--codec",
                "av1",
                "--codec",
                "h264",
                "--rounds",
                "2",
                "--gpu-min-hz",
                "1000000000",
                "--interval-ms",
                "500",
                "--duration",
                "15",
                "--warmup",
                "2",
                "--output",
                str(source / "out"),
            ),
            source,
        )
    )
    assert [workload.name for workload in options.workloads] == ["av1", "h264"]
    assert (options.rounds, options.gpu_minimum_hz, options.interval_ms) == (
        2,
        1000000000,
        500,
    )
    assert (options.duration_seconds, options.warmup_seconds, options.output) == (
        15,
        2,
        source / "out",
    )


def test_parse_custom_video_uses_its_path_without_preset_assumptions(
    source: Path,
) -> None:
    video = source / "movie.webm"
    _ = video.write_bytes(b"custom")
    options = accepted(parse(("--video", str(video), "--warmup", "0"), source))
    assert len(options.workloads) == 1
    assert options.workloads[0].expand_frames is None
    assert (
        options.workloads[0].source == video and options.workloads[0].expected is None
    )


@pytest.mark.parametrize(
    "arguments",
    [
        ("--unknown", "x"),
        ("--rounds",),
        ("--rounds", "0"),
        ("--rounds", "x"),
        ("--rounds", "1", "--rounds", "2"),
        ("--codec", "hevc"),
        ("--codec", "h264", "--codec", "h264"),
        ("--interval-ms", "0"),
        ("--gpu-min-hz", "-1"),
        ("--duration", "NaN"),
        ("--warmup", "-1"),
        ("--duration", "2", "--warmup", "2"),
        ("--video", "x", "--codec", "h264"),
    ],
)
def test_parse_invalid_or_ambiguous_arguments_return_error(
    source: Path, arguments: tuple[str, ...]
) -> None:
    assert isinstance(parse(arguments, source), Rejected)


def test_parse_missing_video_returns_error_before_any_measurement(source: Path) -> None:
    result = parse(("--video", str(source / "missing.webm")), source)
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_parse_compositor_sampling_is_opt_in(source: Path) -> None:
    assert accepted(parse((), source)).compositor_pid is None
    assert accepted(parse(("--compositor-pid", "123"), source)).compositor_pid == 123
    for value in ("0", "-1", "invalid"):
        assert isinstance(parse(("--compositor-pid", value), source), Rejected)
