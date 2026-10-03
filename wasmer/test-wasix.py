# /// script
# requires-python = ">=3.10"
# dependencies = ["websockets>=15,<18"]
# ///
"""Exercise the packaged Wasm: uv run wasmer/test-wasix.py [--engine v8]."""

import argparse
import contextlib
import http.server
import os
from pathlib import Path
import signal
import socket
import ssl
import struct
import subprocess
import threading
import time
import urllib.request

from websockets.sync.client import connect

ROOT = Path(__file__).resolve().parent.parent
BODY = bytes(range(256)) * 1024


def packet(kind, stream, payload=b""):
    return struct.pack("<BI", kind, stream) + payload


class Wisp:
    def __init__(self, url, version=2):
        self.connection = connect(url, subprotocols=["wisp-v2"] if version == 2 else None,
                                  open_timeout=10, close_timeout=2)
        self.ws = self.connection.__enter__()
        self.pending = {}
        info = self.ws.recv(timeout=10)
        if version == 2:
            assert info[:7] == packet(5, 0, b"\x02\x00"), info
            self.ws.send(packet(5, 0, b"\x02\x00"))
            info = self.ws.recv(timeout=10)
        assert info[:5] == packet(3, 0), info
        assert struct.unpack("<I", info[5:])[0] > 0

    def open(self, stream, host, port, kind=1):
        self.ws.send(packet(1, stream, struct.pack("<BH", kind, port) + host.encode()))

    def send(self, stream, data):
        for start in range(0, len(data), 16384):
            self.ws.send(packet(2, stream, data[start : start + 16384]))

    def receive(self, stream):
        while not self.pending.get(stream):
            frame = self.ws.recv(timeout=15)
            assert isinstance(frame, bytes) and len(frame) >= 5, frame
            kind, sid = struct.unpack("<BI", frame[:5])
            if kind in (2, 4):
                self.pending.setdefault(sid, []).append((kind, frame[5:]))
            else:
                assert kind == 3, frame
        return self.pending[stream].pop(0)

    def data(self, stream):
        kind, data = self.receive(stream)
        assert kind == 2, f"Stream {stream} closed: {data.hex()}"
        return data

    def blocked(self, stream, host, port, kind=1):
        self.open(stream, host, port, kind)
        event, reason = self.receive(stream)
        assert (event, reason) == (4, b"\x48"), (event, reason)

    def close(self):
        self.connection.__exit__(None, None, None)


class TLS:
    def __init__(self, mux, stream, context, hostname):
        self.mux, self.stream = mux, stream
        self.incoming, self.outgoing = ssl.MemoryBIO(), ssl.MemoryBIO()
        self.tls = context.wrap_bio(self.incoming, self.outgoing, server_hostname=hostname)
        while True:
            try:
                self.tls.do_handshake()
                self.flush()
                break
            except ssl.SSLWantReadError:
                self.flush()
                self.incoming.write(mux.data(stream))

    def flush(self):
        while self.outgoing.pending:
            self.mux.send(self.stream, self.outgoing.read())

    def send(self, data):
        offset = 0
        while offset < len(data):
            offset += self.tls.write(data[offset:])
            self.flush()

    def read(self):
        while True:
            try:
                data = self.tls.read(16384)
                assert data, "TLS stream ended before response"
                return data
            except ssl.SSLWantReadError:
                self.flush()
                self.incoming.write(self.mux.data(self.stream))


def response(read, expected=None):
    data = b""
    while b"\r\n\r\n" not in data:
        data += read()
    headers, body = data.split(b"\r\n\r\n", 1)
    assert headers.splitlines()[0].split()[1] == b"200", headers
    if expected is not None:
        while len(body) < len(expected):
            body += read()
        assert body == expected, f"Corrupt payload: {len(body)} bytes"
    return headers, body


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


@contextlib.contextmanager
def server(engine, logs, overrides=()):
    port = free_port()
    command = ["wasmer", "run", str(ROOT / "wasmer"), "--net", "--disable-cache",
               f"--{engine}", "--env", f"WISP_SERVER_BIND=127.0.0.1:{port}"]
    for override in overrides:
        command += ["--env", override]
    with logs.open("w") as log:
        proc = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            deadline = time.monotonic() + 40
            while time.monotonic() < deadline:
                assert proc.poll() is None, logs.read_text()
                try:
                    with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=1) as page:
                        assert page.status == 200
                        assert b"WISP WebSocket server" in page.read()
                    break
                except (OSError, urllib.error.URLError):
                    time.sleep(0.1)
            else:
                raise AssertionError("Server startup timed out: " + logs.read_text())
            yield f"ws://127.0.0.1:{port}/"
            assert proc.poll() is None, logs.read_text()
        finally:
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGTERM)
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(proc.pid, signal.SIGKILL)
                    proc.wait()


class Fixture(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Length", str(len(BODY)))
        self.end_headers()
        self.wfile.write(BODY)

    def log_message(self, *_):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=("v8", "cranelift"), default="v8")
    parser.add_argument("--external", action="store_true", help="Also proxy example.com HTTP and verified TLS")
    args = parser.parse_args()
    output = ROOT / ".wasmer-build" / f"smoke-{args.engine}"
    output.mkdir(parents=True, exist_ok=True)
    with server(args.engine, output / "default-policy.log") as url:
        mux = Wisp(url)
        try:
            mux.blocked(1, "127.0.0.1", 80)
            mux.blocked(2, "224.0.0.1", 80)
            mux.blocked(3, "1.1.1.1", 443, kind=2)
            print("PASS: HTTP landing page, WISP v2, blocked loopback/multicast/UDP")
            if args.external:
                mux.open(4, "example.com", 80)
                mux.send(4, b"GET / HTTP/1.0\r\nHost: example.com\r\n\r\n")
                response(lambda: mux.data(4))
                mux.open(5, "example.com", 443)
                tls = TLS(mux, 5, ssl.create_default_context(), "example.com")
                tls.send(b"GET / HTTP/1.0\r\nHost: example.com\r\n\r\n")
                response(tls.read)
                print("PASS: public DNS, HTTP, certificate-verified HTTPS through WISP")
        finally:
            mux.close()

    cert, key = output / "fixture.crt", output / "fixture.key"
    subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                    "-keyout", str(key), "-out", str(cert), "-days", "1",
                    "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost"],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    plain = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    secure = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert, key)
    secure.socket = context.wrap_socket(secure.socket, server_side=True)
    for fixture in (plain, secure):
        threading.Thread(target=fixture.serve_forever, daemon=True).start()
    try:
        overrides = ["WISP_STREAM_ALLOW_LOOPBACK=true", "WISP_STREAM_ALLOW_NON_GLOBAL=true",
                     f"WISP_STREAM_ALLOW_PORTS={plain.server_port},{secure.server_port}"]
        with server(args.engine, output / "fixture-policy.log", overrides) as url:
            for version in (1, 2):
                mux = Wisp(url, version)
                try:
                    # Two simultaneous TCP streams on a single WebSocket.
                    for stream in (1, 2):
                        mux.open(stream, "127.0.0.1", plain.server_port)
                        mux.send(stream, b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
                    for stream in (1, 2):
                        response(lambda stream=stream: mux.data(stream), BODY)
                    mux.open(3, "127.0.0.1", secure.server_port)
                    tls = TLS(mux, 3, ssl.create_default_context(cafile=str(cert)), "localhost")
                    tls.send(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
                    response(tls.read, BODY)
                    print(f"PASS: WISP v{version}, multiplexed HTTP and verified TLS; 3 x 256 KiB intact")
                finally:
                    mux.close()
    finally:
        for fixture in (plain, secure):
            fixture.shutdown()
            fixture.server_close()
    print(f"All {args.engine} checks passed. Logs: {output}")


if __name__ == "__main__":
    main()
