"""module: tests.support.video_formats_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_format_operations_non_video_fd_return_enotty
    test_format_roundtrip_preserves_supported_codec_and_requested_size


"""

import errno
from pathlib import Path

from tests.support.outcome import Rejected, accepted
from tests.support.queue import QueueType
from tests.support.stream import Codec
from tests.support.syscalls import open_fd
from tests.support.video import Video
from tests.support.video_formats import (
    enumerate_formats,
    get_format,
    selection,
    set_format,
)


def test_format_operations_non_video_fd_return_enotty() -> None:
    with accepted(open_fd(Path("/dev/null"))) as owner:
        results = [
            get_format(owner.fd, 10),
            set_format(owner.fd, 10, Codec.H264.fourcc),
            enumerate_formats(owner.fd, 10),
            selection(owner.fd, 9),
        ]
        for result in results:
            assert isinstance(result, Rejected) and result.code == errno.ENOTTY


def test_format_roundtrip_preserves_supported_codec_and_requested_size(
    video: Video,
) -> None:
    requested = accepted(
        set_format(
            video.fd,
            QueueType.OUTPUT,
            Codec.VP9.fourcc,
            width=256,
            height=128,
            size=2 * 1024 * 1024,
        )
    )
    actual = accepted(get_format(video.fd, QueueType.OUTPUT))
    assert actual == requested
    assert (
        actual.width,
        actual.height,
        actual.fourcc,
        actual.planes,
        actual.sizes,
    ) == (256, 128, Codec.VP9.fourcc, 1, (2 * 1024 * 1024,))
