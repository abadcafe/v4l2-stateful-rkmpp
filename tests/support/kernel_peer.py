"""module: kernel_peer; responsibility: own a paired real-kernel test session.

operations: Peer open/close/video/service; entities: Peer;
tests: kernel_peer_tests. The Python caller supplies all service-side replies.
"""

from contextlib import ExitStack
from pathlib import Path
from types import TracebackType

from tests.support.bridge import Session
from tests.support.bridge_abi import ACCEPT
from tests.support.outcome import Ok, Rejected, Result
from tests.support.syscalls import OwnedFd, ioctl, open_fd
from tests.support.video import Video


class Peer:
    def __init__(self, video: Video, service: Session, owned: ExitStack) -> None:
        self.video: Video = video
        self.service: Session = service
        self._owned: ExitStack = owned

    @classmethod
    def open(cls, broker_path: Path, video_path: Path) -> Result[Peer]:
        owned = ExitStack()
        broker = open_fd(broker_path)
        if isinstance(broker, Rejected):
            return broker
        _ = owned.enter_context(broker.value)
        video = Video.open(video_path)
        if isinstance(video, Rejected):
            owned.close()
            return video
        _ = owned.enter_context(video.value)
        service = ioctl(broker.value.fd, ACCEPT)
        if isinstance(service, Rejected):
            owned.close()
            return service
        endpoint = Session(OwnedFd(service.value))
        _ = owned.callback(endpoint.close)
        return Ok(cls(video.value, endpoint, owned))

    def close(self) -> None:
        self._owned.close()

    def __enter__(self) -> Peer:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()
