"""module: site_tests; responsibility: define offline browser media serving.

operations: pytest cases; entities: none; tests: inline page, media, ownership.
Only public HTTP and filesystem operations are used.
"""

from http.client import HTTPConnection
from pathlib import Path
from urllib.parse import urlsplit

from tests.support.browser.site import Site
from tests.support.outcome import Rejected, accepted


def test_open_serves_inline_page_and_binary_media_without_html_file(
    tmp_path: Path,
) -> None:
    video = tmp_path / "video clip.webm"
    _ = video.write_bytes(b"binary media")
    site = accepted(Site.open(tmp_path / "site", (video,)))
    try:
        address = urlsplit(site.origin)
        assert address.hostname is not None
        connection = HTTPConnection(address.hostname, address.port, timeout=2)
        try:
            connection.request("GET", "/")
            response = connection.getresponse()
            assert response.status == 200 and b'<video id="video"' in response.read()
            connection.request("GET", "/video%20clip.webm")
            response = connection.getresponse()
            assert response.status == 200 and response.read() == b"binary media"
        finally:
            connection.close()
        assert not tuple((tmp_path / "site").rglob("*.html"))
    finally:
        assert accepted(site.close()) is None
    assert accepted(site.close()) is None


def test_open_invalid_media_and_duplicate_names_return_error(tmp_path: Path) -> None:
    assert isinstance(
        Site.open(tmp_path / "missing", (tmp_path / "no.webm",)), Rejected
    )
    video = tmp_path / "video.webm"
    _ = video.write_bytes(b"video")
    assert isinstance(Site.open(tmp_path / "duplicate", (video, video)), Rejected)
