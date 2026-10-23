"""module: tests.tools.performance.cpu_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_parse_stat_handles_process_names_with_parentheses
    test_parse_stat_malformed_fields_are_rejected
    test_account_uses_owned_browser_descendants_and_one_core_percent
    test_account_ignores_reused_pid_and_new_process_without_baseline
    test_account_invalid_sampling_interval_is_rejected

CPU accounting defines one core as 100% and excludes unrelated browsers.
"""

import errno
import math
from pathlib import Path

import pytest

from tests.support.outcome import Rejected, accepted
from tests.tools.performance.cpu import Process, account, parse_stat, processes


def test_parse_stat_handles_process_names_with_parentheses() -> None:
    fields = ["R", "7", *["0"] * 9, "20", "10", *["0"] * 6, "123"]
    parsed = accepted(
        parse_stat("5 (a name)) " + " ".join(fields), b"chromium\0--type=renderer\0")
    )
    assert (parsed.parent, parsed.start, parsed.ticks, parsed.name, parsed.kind) == (
        7,
        123,
        30,
        "a name)",
        "renderer",
    )


def test_parse_stat_malformed_fields_are_rejected() -> None:
    for raw in ("missing", "1 (x) R", "1 (x) " + " ".join(["NaN"] * 20)):
        assert isinstance(parse_stat(raw, b""), Rejected)


def test_account_uses_owned_browser_descendants_and_one_core_percent() -> None:
    old = {
        1: Process(parent=0, start=1, ticks=0, name="chromium", kind="browser"),
        2: Process(parent=1, start=2, ticks=10, name="chromium", kind="browser"),
        3: Process(parent=2, start=3, ticks=20, name="chromium", kind="renderer"),
        4: Process(parent=0, start=4, ticks=30, name="chromium", kind="browser"),
        5: Process(
            parent=0, start=5, ticks=40, name="v4l2-stateful-rkmpp", kind="browser"
        ),
        6: Process(parent=0, start=6, ticks=50, name="desktop-shell", kind="browser"),
        7: Process(parent=0, start=7, ticks=60, name="desktop-shell", kind="browser"),
        8: Process(parent=7, start=8, ticks=70, name="desktop-agent", kind="browser"),
    }
    now = {
        pid: Process(
            parent=p.parent,
            start=p.start,
            ticks=p.ticks + 100,
            name=p.name,
            kind=p.kind,
        )
        for pid, p in old.items()
    }
    groups = accepted(account(now, old, 1.0, 100, 1, 7))
    assert groups == {
        "chromium": 300.0,
        "browser": 200.0,
        "renderer": 100.0,
        "gpu-process": 0.0,
        "other-chromium": 0.0,
        "desktop-compositor": 100.0,
    }


def test_account_ignores_reused_pid_and_new_process_without_baseline() -> None:
    old = {
        5: Process(
            parent=0, start=1, ticks=100, name="v4l2-stateful-rkmpp", kind="browser"
        )
    }
    now = {
        5: Process(
            parent=0, start=2, ticks=1, name="v4l2-stateful-rkmpp", kind="browser"
        )
    }
    assert accepted(account(now, old, 1, 100, 5, 7))["chromium"] == 0


def test_account_includes_owned_processes_independently_of_executable_name() -> None:
    old = {
        1: Process(parent=0, start=1, ticks=10, name="chrome", kind="browser"),
        2: Process(parent=1, start=2, ticks=20, name="renamed-worker", kind="renderer"),
        3: Process(parent=1, start=3, ticks=30, name="chrome_crashpad", kind="utility"),
        4: Process(parent=0, start=4, ticks=40, name="chromium", kind="browser"),
    }
    now = {
        pid: process.model_copy(update={"ticks": process.ticks + 100})
        for pid, process in old.items()
    }
    groups = accepted(account(now, old, 1, 100, 1, None))
    assert groups == {
        "chromium": 300.0,
        "browser": 100.0,
        "renderer": 100.0,
        "gpu-process": 0.0,
        "other-chromium": 100.0,
    }


def test_account_handles_deep_process_trees_without_recursion() -> None:
    old = {
        pid: Process(parent=pid - 1, start=pid, ticks=0, name="chrome", kind="browser")
        for pid in range(1, 5001)
    }
    now = {pid: process.model_copy(update={"ticks": 1}) for pid, process in old.items()}
    groups = accepted(account(now, old, 1, 100, 1, None))
    assert groups["chromium"] == 5000.0
    assert groups["browser"] == 5000.0


def test_account_invalid_sampling_interval_is_rejected() -> None:
    for elapsed, ticks in (
        (0.0, 100),
        (-1.0, 100),
        (1.0, 0),
        (math.nan, 100),
        (math.inf, 100),
    ):
        result = account({}, {}, elapsed, ticks, 1, 3)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL


def _process(proc: Path, pid: int, children: str = "", kind: str = "browser") -> None:
    directory = proc / str(pid)
    (directory / "task" / str(pid)).mkdir(parents=True)
    fields = ["R", "0", *["0"] * 9, "20", "10", *["0"] * 6, "123"]
    _ = (directory / "stat").write_text(f"{pid} (chromium) " + " ".join(fields))
    _ = (directory / "cmdline").write_bytes(
        b"chromium\0--type=" + kind.encode() + b"\0"
    )
    _ = (directory / "task" / str(pid) / "children").write_text(children)


def test_processes_reads_only_owned_tree_and_required_compositor(
    tmp_path: Path,
) -> None:
    _process(tmp_path, 1, "2 3")
    _process(tmp_path, 2, "4", "renderer")
    _process(tmp_path, 4, kind="gpu-process")
    _process(tmp_path, 9)
    # An inaccessible unrelated PID must never be consulted.
    (tmp_path / "99").mkdir()
    counters = accepted(processes(1, 9, tmp_path))
    assert set(counters) == {1, 2, 4, 9}
    assert counters[4].kind == "gpu-process"


def test_processes_includes_children_spawned_by_non_main_threads(
    tmp_path: Path,
) -> None:
    _process(tmp_path, 1)
    _process(tmp_path, 2)
    _process(tmp_path, 9)
    thread = tmp_path / "1/task/10"
    thread.mkdir()
    _ = (thread / "children").write_text("2")
    assert set(accepted(processes(1, 9, tmp_path))) == {1, 2, 9}


def test_processes_required_process_missing_is_an_error(tmp_path: Path) -> None:
    _process(tmp_path, 1)
    result = processes(1, 9, tmp_path)
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_processes_required_counters_permission_denied_is_not_omitted(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _process(tmp_path, 1)
    _process(tmp_path, 9)
    read = Path.read_text

    def denied(
        path: Path, encoding: str | None = None, errors: str | None = None
    ) -> str:
        if path == tmp_path / "1/stat":
            raise PermissionError(errno.EACCES, "required counter")
        return read(path, encoding=encoding, errors=errors)

    monkeypatch.setattr(Path, "read_text", denied)
    result = processes(1, 9, tmp_path)
    assert isinstance(result, Rejected) and result.code == errno.EACCES


def test_processes_without_compositor_reads_only_browser_tree(tmp_path: Path) -> None:
    _process(tmp_path, 1, "2")
    _process(tmp_path, 2)
    (tmp_path / "99").mkdir()
    values = accepted(processes(1, None, tmp_path))
    assert set(values) == {1, 2}
    groups = accepted(account(values, values, 1.0, 100, 1, None))
    assert "desktop-compositor" not in groups
    assert groups["chromium"] == 0


@pytest.mark.parametrize("children", ["NaN", "0", "-1"])
def test_processes_invalid_child_inventory_is_rejected(
    tmp_path: Path, children: str
) -> None:
    _process(tmp_path, 1, children)
    _process(tmp_path, 9)
    result = processes(1, 9, tmp_path)
    assert isinstance(result, Rejected) and result.code == errno.EINVAL
