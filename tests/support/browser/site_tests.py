"""module: site_tests; responsibility: define offline browser media serving.

operations: pytest cases; entities: none; tests: inline page, media, ownership.
Only public HTTP and filesystem operations are used.
"""

import socket
import threading
import time
from http.client import HTTPConnection
from pathlib import Path
from urllib.parse import urlsplit

from tests.support.browser.site import Site
from tests.support.outcome import Rejected, accepted


def test_close_releases_clients_with_unfinished_http_requests(tmp_path: Path) -> None:
    before = set(threading.enumerate())
    site = accepted(Site.open(tmp_path / "site", ()))
    address = urlsplit(site.origin)
    assert address.hostname is not None and address.port is not None
    with socket.create_connection((address.hostname, address.port), timeout=2) as idle:
        with socket.create_connection(
            (address.hostname, address.port), timeout=2
        ) as partial:
            partial.sendall(b"GET / HTTP/1.0\r\nHost: localhost\r\n")
            deadline = time.monotonic() + 2
            workers = set(threading.enumerate()) - before
            while len(workers) < 3 and time.monotonic() < deadline:
                time.sleep(0.001)
                workers = set(threading.enumerate()) - before
            assert len(workers) == 3
            try:
                assert accepted(site.close()) is None
                for worker in workers:
                    worker.join(timeout=0.2)
                assert all(not worker.is_alive() for worker in workers)
                assert idle.recv(1) == b""
            finally:
                # Always release the sockets, including when this regression
                # exposes leaked request threads in the original server.
                partial.close()
                idle.close()
                accepted(site.close())
                for worker in workers:
                    worker.join(timeout=2)


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
