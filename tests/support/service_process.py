"""module: service_process; responsibility: lease an owned or borrowed daemon.

operations: Lease start/borrow/running/close; entities: Lease;
tests: service_process_tests, browser acceptance. Only owned processes are stopped.
"""

import errno
import math
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import BinaryIO

from tests.support.diagnostics import query
from tests.support.outcome import Ok, Rejected, Result
from tests.support.service_runtime import Running, sample
from tests.support.syscalls import OwnedFd


class Lease:
    def __init__(
        self,
        running: Running,
        descriptor: OwnedFd,
        process: subprocess.Popen[bytes] | None = None,
        log: BinaryIO | None = None,
    ) -> None:
        self._running: Running = running
        self._descriptor: OwnedFd | None = descriptor
        self._process: subprocess.Popen[bytes] | None = process
        self._log: BinaryIO | None = log

    @classmethod
    def borrow(cls, running: Running) -> Result[Lease]:
        try:
            descriptor = OwnedFd(os.pidfd_open(running.identity.pid))
        except OSError as error:
            return Rejected(error.errno or 0, f"pin service process identity: {error}")
        confirmed = sample(running)
        if isinstance(confirmed, Rejected):
            descriptor.close()
            return confirmed
        return Ok(cls(running, descriptor))

    @classmethod
    def start(
        cls,
        binary: Path,
        control: Path,
        endpoint: Path,
        log_path: Path,
        *,
        startup_timeout: float = 10,
    ) -> Result[Lease]:
        if not math.isfinite(startup_timeout) or startup_timeout <= 0:
            return Rejected(errno.EINVAL, "invalid service startup deadline")
        try:
            log = log_path.open("wb")
        except OSError as error:
            return Rejected(error.errno or 0, f"create service log: {error}")
        process: subprocess.Popen[bytes] | None = None
        result: Result[Lease] | None = None
        try:
            try:
                process = subprocess.Popen(
                    [str(binary), "serve", str(control), str(endpoint)],
                    stdout=log,
                    stderr=log,
                )
            except OSError as error:
                result = Rejected(error.errno or 0, f"start service {binary}: {error}")
            if process is not None:
                result = _ready(process, endpoint, startup_timeout)
                if isinstance(result, Ok):
                    result.value._process = process
                    result.value._log = log
        finally:
            if result is None or isinstance(result, Rejected):
                stopped = _stop(process, log)
                if isinstance(stopped, Rejected):
                    if isinstance(result, Rejected):
                        result = Rejected(
                            result.code,
                            result.operation + "; cleanup: " + stopped.operation,
                        )
                    else:
                        print(stopped.operation, file=sys.stderr)
        assert result is not None
        return result

    @property
    def running(self) -> Running:
        return self._running

    def close(self) -> Result[None]:
        if self._descriptor is None:
            return Ok(None)
        try:
            result = _stop(self._process, self._log)
        finally:
            self._descriptor.close()
            self._descriptor = None
        return result


def _ready(
    process: subprocess.Popen[bytes], endpoint: Path, timeout: float
) -> Result[Lease]:
    deadline = time.monotonic() + timeout
    while process.poll() is None and time.monotonic() < deadline:
        status = query(endpoint)
        if isinstance(status, Ok):
            if status.value.identity.pid != process.pid:
                return Rejected(
                    errno.EBUSY, "service endpoint belongs to another process"
                )
            if status.value.state == "ready":
                return Lease.borrow(
                    Running(identity=status.value.identity, endpoint=endpoint)
                )
        elif status.code not in (errno.ENOENT, errno.ECONNREFUSED):
            return status
        time.sleep(0.02)
    return Rejected(
        errno.EIO, "owned service failed to become ready; inspect its service.log"
    )


def _stop(
    process: subprocess.Popen[bytes] | None, log: BinaryIO | None
) -> Result[None]:
    failure: Rejected | None = None
    try:
        if process is not None:
            if process.poll() is None:
                process.terminate()
            try:
                result = process.wait(timeout=10)
                if result != 0:
                    failure = Rejected(
                        errno.EIO, f"owned service exited with status {result}"
                    )
            except subprocess.TimeoutExpired:
                process.kill()
                _ = process.wait(timeout=5)
                failure = Rejected(errno.ETIMEDOUT, "owned service required SIGKILL")
    except OSError as error:
        failure = Rejected(error.errno or 0, f"stop owned service: {error}")
    finally:
        if log is not None:
            log.close()
    return failure if failure is not None else Ok(None)
