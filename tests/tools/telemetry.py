"""module: telemetry; responsibility: sample owned decode workload utilization.

operations: gpu_load, mpp_load, Sampler open/read; entities: Sampler;
tests: telemetry_tests; used only by the standalone measurement tool.
"""

import errno
import os
import re
import time
from pathlib import Path

from tests.support import service_runtime
from tests.support.outcome import Ok, Rejected, Result
from tests.support.service_runtime import Running
from tests.tools import cpu, report


def gpu_load(raw: str) -> Result[tuple[float, int]]:
    match = re.fullmatch(r"(\d+(?:\.\d+)?)@(\d+)Hz\s*", raw)
    if match is None:
        return Rejected(errno.EINVAL, "unrecognized GPU load")
    percent, frequency = float(match[1]), int(match[2])
    if percent > 100 or frequency == 0:
        return Rejected(errno.EINVAL, "GPU load outside limits")
    return Ok((percent, frequency))


def mpp_load(raw: str) -> Result[dict[str, float]]:
    values = {
        match[1]: float(match[2])
        for match in re.finditer(r"(\S+)\s+load:\s+(\d+\.\d+)%\s+utilization:", raw)
    }
    if not values or any(value > 100 for value in values.values()):
        return Rejected(errno.EINVAL, "missing or invalid MPP load")
    return Ok(values)


class Sampler:
    def __init__(
        self,
        browser_pid: int,
        bridge: Running,
        desktop_pid: int | None,
        initial: cpu.Processes,
        bridge_cpu: float,
    ) -> None:
        self._browser_pid: int = browser_pid
        self._bridge: Running = bridge
        self._bridge_cpu: float = bridge_cpu
        self._desktop_pid: int | None = desktop_pid
        self._start: float = time.monotonic()
        self._previous_time: float = self._start
        self._previous: cpu.Processes = initial

    @classmethod
    def open(
        cls, browser_pid: int, bridge: Running, desktop_pid: int | None = None
    ) -> Result[Sampler]:
        if browser_pid <= 0 or (desktop_pid is not None and desktop_pid <= 0):
            return Rejected(errno.EINVAL, "invalid workload process identity")
        counters = cpu.processes(browser_pid, desktop_pid)
        if isinstance(counters, Rejected):
            return counters
        if browser_pid not in counters.value or (
            desktop_pid is not None and desktop_pid not in counters.value
        ):
            return Rejected(errno.ESRCH, "workload process disappeared")
        status = service_runtime.sample(bridge)
        if isinstance(status, Rejected):
            return status
        return Ok(
            cls(
                browser_pid,
                bridge,
                desktop_pid,
                counters.value,
                status.value.cpu_seconds,
            )
        )

    def read(self) -> Result[report.Sample]:
        now = time.monotonic()
        current = cpu.processes(self._browser_pid, self._desktop_pid)
        if isinstance(current, Rejected):
            return current
        percentages = cpu.account(
            current.value,
            self._previous,
            now - self._previous_time,
            int(os.sysconf("SC_CLK_TCK")),
            self._browser_pid,
            self._desktop_pid,
        )
        if isinstance(percentages, Rejected):
            return percentages
        status = service_runtime.sample(self._bridge)
        if isinstance(status, Rejected):
            return status
        delta = status.value.cpu_seconds - self._bridge_cpu
        if delta < 0:
            return Rejected(
                errno.EINVAL, "service CPU counter decreased within process lifetime"
            )
        percentages.value["bridge"] = 100 * delta / (now - self._previous_time)
        try:
            gpu = gpu_load(
                Path("/sys/class/devfreq/fb000000.gpu-panthor/load").read_text()
            )
            mpp = mpp_load(Path("/proc/mpp_service/load").read_text())
        except OSError as error:
            return Rejected(error.errno or 0, "hardware counter read")
        if isinstance(gpu, Rejected):
            return gpu
        if isinstance(mpp, Rejected):
            return mpp
        self._bridge_cpu = status.value.cpu_seconds
        self._previous = current.value
        self._previous_time = now
        return Ok(
            report.Sample(
                elapsed=now - self._start,
                gpu_percent=gpu.value[0],
                gpu_frequency=gpu.value[1],
                mpp=mpp.value,
                cpu=percentages.value,
            )
        )
