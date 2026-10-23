"""module: browser.site; responsibility: host owned offline playback media.

operations: Site open/origin/close; entities: Site, _Server, _Handler;
tests: site_tests.
HTML stays inline and all hosted files are explicit caller-owned inputs.
"""

import errno
import socket
import threading
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import final, override

from tests.support.outcome import Ok, Rejected, Result

type _Request = socket.socket | tuple[bytes, socket.socket]


class _Server(ThreadingHTTPServer):
    # server_close must join every request owner before Site releases the
    # media lifetime. The stdlib default detaches daemon request threads.
    daemon_threads: bool = False

    def __init__(self, directory: Path) -> None:
        self._connections: set[socket.socket] = set()
        self._lock: threading.Lock = threading.Lock()
        super().__init__(("127.0.0.1", 0), partial(_Handler, directory=str(directory)))

    @override
    def process_request(
        self, request: _Request, client_address: tuple[str, int]
    ) -> None:
        # Register before starting the thread so shutdown cannot miss a
        # request that has been accepted but has not started reading yet.
        assert isinstance(request, socket.socket)
        with self._lock:
            self._connections.add(request)
        super().process_request(request, client_address)

    @override
    def shutdown_request(self, request: _Request) -> None:
        assert isinstance(request, socket.socket)
        with self._lock:
            self._connections.discard(request)
        super().shutdown_request(request)

    def close_connections(self) -> None:
        # The accept loop has stopped. Wake blocked readers/writers without
        # closing descriptors still owned by their request threads.
        with self._lock:
            for connection in self._connections:
                try:
                    connection.shutdown(socket.SHUT_RDWR)
                except OSError:
                    # A client may have disconnected while shutdown began.
                    pass


@final
class _Handler(SimpleHTTPRequestHandler):
    _page: bytes = b"""<!doctype html>
<meta charset="utf-8"><title>RKMPP playback</title>
<style>body{margin:0;background:#202020}video{display:block;width:768px;max-width:100vw}</style>
<video id="video" muted controls></video>
"""

    @override
    def do_GET(self) -> None:
        try:
            if self.path == "/":
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(self._page)))
                self.end_headers()
                _ = self.wfile.write(self._page)
            else:
                super().do_GET()
        except BrokenPipeError, ConnectionResetError:
            return

    @override
    def log_message(self, format: str, *args: object) -> None:
        # The caller records playback/CDP failures; HTTP downloads stay quiet.
        return


class Site:
    def __init__(self, server: _Server, thread: threading.Thread) -> None:
        self._server: _Server = server
        self._thread: threading.Thread = thread
        self._closed: bool = False

    @classmethod
    def open(cls, directory: Path, media: tuple[Path, ...]) -> Result[Site]:
        if any(not file.is_file() for file in media):
            return Rejected(errno.ENOENT, "missing browser media input")
        if len({file.name for file in media}) != len(media):
            return Rejected(errno.EEXIST, "browser media names must be unique")
        try:
            directory.mkdir()
            for file in media:
                (directory / file.name).symlink_to(file.resolve())
            server = _Server(directory)
        except OSError as error:
            return Rejected(error.errno or 0, f"host browser media: {error}")
        thread = threading.Thread(target=server.serve_forever, name="browser-media")
        thread.start()
        return Ok(cls(server, thread))

    @property
    def origin(self) -> str:
        return f"http://127.0.0.1:{self._server.server_port}"

    def close(self) -> Result[None]:
        if self._closed:
            return Ok(None)
        self._server.shutdown()
        self._server.close_connections()
        self._server.server_close()
        self._thread.join(timeout=5)
        if self._thread.is_alive():
            return Rejected(errno.ETIMEDOUT, "browser media server did not stop")
        self._closed = True
        return Ok(None)
