"""module: tests.support.video_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_open_missing_device_returns_rejected
    test_capability_and_control_non_video_fd_report_enotty
    test_events_empty_subscribed_video_returns_empty_list


"""

import errno
from pathlib import Path

import pytest

from tests.support.outcome import Rejected, accepted
from tests.support.video import Video


@pytest.mark.parametrize("kind", [-1, 1 << 32, 1 << 128])
def test_subscribe_rejects_unrepresentable_event_type(kind: int) -> None:
    with accepted(Video.open(Path("/dev/null"))) as video:
        result = video.subscribe(kind)
        assert isinstance(result, Rejected) and result.code == errno.EINVAL


def test_open_missing_device_returns_rejected(tmp_path: Path) -> None:
    result = Video.open(tmp_path / "missing")
    assert isinstance(result, Rejected) and result.code == errno.ENOENT


def test_capability_and_control_non_video_fd_report_enotty() -> None:
    with accepted(Video.open(Path("/dev/null"))) as video:
        results = [
            video.capability(),
            video.subscribe(5),
            video.events(),
            video.stop(),
            video.start(),
        ]
        for result in results:
            assert isinstance(result, Rejected) and result.code == errno.ENOTTY
        assert video.poll(0) > 0


def test_events_empty_subscribed_video_returns_empty_list(video: Video) -> None:
    accepted(video.subscribe(5))
    assert accepted(video.events()) == []
