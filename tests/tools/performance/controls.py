"""module: controls; responsibility: lease reversible measurement parameters.

operations: Paths, Settings, Controls open/settings/close; entities: three;
tests: controls_tests. Only explicit frequency requests alter the GPU policy.
"""

import errno
import sys
from pathlib import Path

from pydantic import BaseModel

from tests.support.outcome import Ok, Rejected, Result


class Paths(BaseModel, frozen=True):
    minimum: Path
    maximum: Path
    interval: Path


class Settings(BaseModel, frozen=True):
    original_minimum_hz: int
    requested_minimum_hz: int | None
    original_interval_ms: int
    interval_ms: int


_PATHS = Paths(
    minimum=Path("/sys/class/devfreq/fb000000.gpu-panthor/min_freq"),
    maximum=Path("/sys/class/devfreq/fb000000.gpu-panthor/max_freq"),
    interval=Path("/proc/mpp_service/load_interval"),
)


class Controls:
    def __init__(
        self, paths: Paths, settings: Settings, minimum: str, interval: str
    ) -> None:
        self._paths: Paths = paths
        self._settings: Settings = settings
        self._minimum: str = minimum
        self._interval: str = interval
        self._frequency_changed: bool = False
        self._interval_changed: bool = False

    @classmethod
    def open(
        cls, minimum_hz: int | None, interval_ms: int, paths: Paths = _PATHS
    ) -> Result[Controls]:
        if interval_ms <= 0 or minimum_hz is not None and minimum_hz <= 0:
            return Rejected(errno.EINVAL, "invalid measurement controls")
        try:
            requested_minimum = str(minimum_hz) if minimum_hz is not None else None
            requested_interval = str(interval_ms)
        except ValueError:
            return Rejected(errno.EINVAL, "measurement controls exceed decimal limit")
        try:
            minimum = paths.minimum.read_text()
            interval = paths.interval.read_text()
            maximum = paths.maximum.read_text()
        except OSError as error:
            return Rejected(error.errno or 0, f"read measurement controls: {error}")
        try:
            old_minimum, old_interval, upper = int(minimum), int(interval), int(maximum)
        except ValueError:
            return Rejected(errno.EPROTO, "invalid measurement control values")
        if minimum_hz is not None and minimum_hz > upper:
            return Rejected(errno.EINVAL, "requested GPU minimum exceeds maximum")
        owner = cls(
            paths,
            Settings(
                original_minimum_hz=old_minimum,
                requested_minimum_hz=minimum_hz,
                original_interval_ms=old_interval,
                interval_ms=interval_ms,
            ),
            minimum,
            interval,
        )
        result: Result[Controls] | None = None
        try:
            if requested_minimum is not None:
                owner._frequency_changed = True
                _ = paths.minimum.write_text(requested_minimum)
            owner._interval_changed = True
            _ = paths.interval.write_text(requested_interval)
            result = Ok(owner)
        except OSError as error:
            message = f"write measurement controls: {error}; apply ../armbian-binary-pkgbuilds/v4l2-stateful-rkmpp/70-v4l2-stateful-rkmpp.rules and use the video group"
            result = Rejected(error.errno or 0, message)
        except KeyboardInterrupt:
            result = Rejected(errno.ECANCELED, "measurement control setup interrupted")
        finally:
            if not isinstance(result, Ok):
                restored = owner.close()
                if isinstance(restored, Rejected):
                    if isinstance(result, Rejected):
                        result = Rejected(
                            result.code,
                            result.operation + "; rollback: " + restored.operation,
                        )
                    else:
                        print(restored.operation, file=sys.stderr)
        assert result is not None
        return result

    @property
    def settings(self) -> Settings:
        return self._settings

    def close(self) -> Result[None]:
        failure: Rejected | None = None
        if self._interval_changed:
            try:
                _ = self._paths.interval.write_text(self._interval)
                self._interval_changed = False
            except OSError as error:
                failure = Rejected(
                    error.errno or 0, f"restore MPP sampling period: {error}"
                )
        if self._frequency_changed:
            try:
                _ = self._paths.minimum.write_text(self._minimum)
                self._frequency_changed = False
            except OSError as error:
                failure = Rejected(
                    error.errno or 0, f"restore GPU minimum frequency: {error}"
                )
        return failure if failure is not None else Ok(None)
