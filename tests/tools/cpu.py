"""module: cpu; responsibility: account CPU time for owned process trees.

operations: parse_stat, processes, account; entities: Process, Processes;
tests: cpu_tests, direct performance tool runs; unit is one CPU core = 100%.
"""

import errno
import re
from pathlib import Path

from pydantic import BaseModel

from tests.support.outcome import Ok, Rejected, Result


class Process(BaseModel):
    parent: int
    start: int
    ticks: int
    name: str
    kind: str


type Processes = dict[int, Process]

_PROC = Path("/proc")


def parse_stat(raw: str, arguments: bytes) -> Result[Process]:
    begin = raw.find("(")
    end = raw.rfind(")")
    if begin < 0 or end <= begin:
        return Rejected(errno.EINVAL, "invalid /proc stat name")
    fields = raw[end + 1 :].split()
    if len(fields) < 20:
        return Rejected(errno.EINVAL, "truncated /proc stat counters")
    try:
        parent, start, ticks = (
            int(fields[1]),
            int(fields[19]),
            int(fields[11]) + int(fields[12]),
        )
    except ValueError:
        return Rejected(errno.EINVAL, "invalid /proc stat counters")
    match = re.search(rb"(?:^|[\s\0])--type=([^\s\0]+)", arguments)
    return Ok(
        Process(
            parent=parent,
            start=start,
            ticks=ticks,
            name=raw[begin + 1 : end],
            kind=match[1].decode(errors="replace") if match else "browser",
        )
    )


def processes(
    browser_pid: int, desktop_pid: int | None = None, proc: Path = _PROC
) -> Result[Processes]:
    if browser_pid <= 0 or (desktop_pid is not None and desktop_pid <= 0):
        return Rejected(errno.EINVAL, "invalid workload process identity")
    values: Processes = {}
    pending = [browser_pid]
    if desktop_pid is not None:
        pending.append(desktop_pid)
    visited: set[int] = set()
    while pending:
        pid = pending.pop()
        if pid in visited:
            continue
        visited.add(pid)
        entry = proc / str(pid)
        try:
            raw = (entry / "stat").read_text()
            arguments = (entry / "cmdline").read_bytes()
            if pid != desktop_pid or pid == browser_pid:
                for task in (entry / "task").iterdir():
                    try:
                        children = (task / "children").read_text().split()
                    except FileNotFoundError, ProcessLookupError:
                        continue
                    try:
                        identified = tuple(int(child) for child in children)
                    except ValueError:
                        return Rejected(errno.EINVAL, "invalid child process inventory")
                    if any(child <= 0 for child in identified):
                        return Rejected(errno.EINVAL, "invalid child process identity")
                    pending.extend(identified)
        except (FileNotFoundError, ProcessLookupError) as error:
            if pid not in (browser_pid, desktop_pid):
                continue
            return Rejected(
                error.errno or errno.ESRCH, "required workload process disappeared"
            )
        except OSError as error:
            return Rejected(
                error.errno or 0, f"read required process counters {entry}: {error}"
            )
        parsed = parse_stat(raw, arguments)
        if isinstance(parsed, Rejected):
            return parsed
        values[pid] = parsed.value
    return Ok(values)


def _descendants(values: Processes, root: int) -> set[int]:
    descendants = {root}
    while True:
        more = {pid for pid, process in values.items() if process.parent in descendants}
        if more <= descendants:
            return descendants
        descendants |= more


def account(
    now: Processes,
    previous: Processes,
    elapsed: float,
    ticks: int,
    browser_pid: int,
    desktop_pid: int | None,
) -> Result[dict[str, float]]:
    if elapsed <= 0 or ticks <= 0:
        return Rejected(errno.EINVAL, "invalid CPU interval")
    descendants = _descendants(now, browser_pid)
    groups = dict.fromkeys(
        [
            "chromium",
            "browser",
            "renderer",
            "gpu-process",
            "other-chromium",
        ],
        0.0,
    )
    if desktop_pid is not None:
        groups["desktop-compositor"] = 0.0
    for pid, p in now.items():
        old = previous.get(pid)
        if old is None or old.start != p.start:
            continue
        delta = p.ticks - old.ticks
        if delta < 0:
            return Rejected(
                errno.EINVAL, "CPU counter decreased within process lifetime"
            )
        percent = 100 * delta / ticks / elapsed
        if pid == desktop_pid:
            groups["desktop-compositor"] += percent
        elif pid in descendants and p.name == "chromium":
            groups["chromium"] += percent
            groups[
                p.kind
                if p.kind in ("browser", "renderer", "gpu-process")
                else "other-chromium"
            ] += percent
    return Ok(groups)
