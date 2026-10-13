"""module: test_controls; responsibility: specify kernel V4L2 control negotiation.

operations: pytest cases; entities: none; tests: capture minimum, profiles and invalid menus.
The service peer is Python; no production daemon is involved.
"""

import ctypes
import errno

import pytest

from tests.support.control_abi import Control, QueryControl, QueryMenu
from tests.support.outcome import Rejected, accepted
from tests.support.syscalls import command, ioctl
from tests.support.video import Video


def test_control_capture_minimum_is_readonly_eighteen_buffers(video: Video) -> None:
    query = QueryControl(id=0x980927)
    assert (
        accepted(ioctl(video.fd, command(3, "V", 36, ctypes.sizeof(query)), query)) == 0
    )
    assert (query.minimum, query.maximum, query.default, query.step) == (18, 18, 18, 1)
    assert query.flags & 4
    value = Control(id=query.id)
    assert (
        accepted(ioctl(video.fd, command(3, "V", 27, ctypes.sizeof(value)), value)) == 0
    )
    assert value.value == 18
    value.value = 17
    result = ioctl(video.fd, command(3, "V", 28, ctypes.sizeof(value)), value)
    assert isinstance(result, Rejected) and result.code == errno.EACCES


@pytest.mark.parametrize(
    ("id", "maximum", "default", "excluded"),
    [
        (0x990A6B, 4, 4, 3),
        (0x990B00, 2, 0, 1),
        (0x990B67, 2, 0, None),
    ],
)
def test_control_profile_menu_roundtrip_and_excluded_values(
    video: Video,
    id: int,
    maximum: int,
    default: int,
    excluded: int | None,
) -> None:
    query = QueryControl(id=id)
    assert (
        accepted(ioctl(video.fd, command(3, "V", 36, ctypes.sizeof(query)), query)) == 0
    )
    assert (query.kind, query.minimum, query.maximum, query.default) == (
        3,
        0,
        maximum,
        default,
    )
    value = Control(id=id)
    assert (
        accepted(ioctl(video.fd, command(3, "V", 27, ctypes.sizeof(value)), value)) == 0
    )
    assert value.value == default
    value.value = maximum
    assert (
        accepted(ioctl(video.fd, command(3, "V", 28, ctypes.sizeof(value)), value)) == 0
    )
    value.value = -1
    assert (
        accepted(ioctl(video.fd, command(3, "V", 27, ctypes.sizeof(value)), value)) == 0
    )
    assert value.value == maximum
    for index in range(maximum + 2):
        menu = QueryMenu(id=id, index=index)
        result = ioctl(video.fd, command(3, "V", 37, ctypes.sizeof(menu)), menu)
        if index == excluded or index > maximum:
            assert isinstance(result, Rejected) and result.code == errno.EINVAL
        else:
            assert accepted(result) == 0
            assert bytes(menu.name).split(b"\0", 1)[0]
    value.value = excluded if excluded is not None else maximum + 1
    result = ioctl(video.fd, command(3, "V", 28, ctypes.sizeof(value)), value)
    expected = errno.EINVAL if excluded is not None else errno.ERANGE
    assert isinstance(result, Rejected) and result.code == expected


def test_control_unknown_id_is_rejected_for_query_get_and_set(video: Video) -> None:
    query = QueryControl(id=0x980FFF)
    value = Control(id=query.id)
    for number, payload in ((36, query), (27, value), (28, value)):
        result = ioctl(
            video.fd, command(3, "V", number, ctypes.sizeof(payload)), payload
        )
        assert isinstance(result, Rejected) and result.code == errno.EINVAL
