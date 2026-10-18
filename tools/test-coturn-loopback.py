"""Real coturn REST-auth/UDP+TCP relay checks, loopback only, no root/services.

Uses a private throwaway config and random test secrets. This is a protocol test,
not proof of browser media, NAT traversal, latency, or public deployment security.
"""
import base64
import argparse
import hashlib
import hmac
import os
from pathlib import Path
import secrets
import shutil
import socket
import struct
import subprocess
import tempfile
import threading
import time

MAGIC = 0x2112A442


def attribute(kind, value):
    return struct.pack("!HH", kind, len(value)) + value + b"\0" * (-len(value) % 4)


def packet(kind, attributes, key=None):
    transaction = secrets.token_bytes(12)
    body = b"".join(attribute(t, value) for t, value in attributes)
    header = struct.pack("!HHI12s", kind, len(body) + (24 if key else 0), MAGIC, transaction)
    if key:
        body += attribute(0x0008, hmac.new(key, header + body, hashlib.sha1).digest())
    return header + body, transaction


def parse(data, transaction=None, key=None):
    if len(data) < 20:
        raise AssertionError("Truncated STUN response")
    kind, length, magic, received = struct.unpack("!HHI12s", data[:20])
    if magic != MAGIC or len(data) != length + 20 or (transaction is not None and received != transaction):
        raise AssertionError("Invalid STUN response framing/transaction")
    result = {}
    offset = 20
    verified = False
    while offset < len(data):
        attr, size = struct.unpack("!HH", data[offset:offset+4])
        value = data[offset+4:offset+4+size]
        if len(value) != size or attr in result:
            raise AssertionError("Invalid STUN attribute")
        if attr == 0x0008 and key:
            signed = bytearray(data[:offset])
            struct.pack_into("!H", signed, 2, offset - 20 + 24)
            if not hmac.compare_digest(value, hmac.new(key, signed, hashlib.sha1).digest()):
                raise AssertionError("TURN response integrity check failed")
            verified = True
        result[attr] = value
        offset += 4 + size + (-size % 4)
    if key and not verified:
        raise AssertionError("Missing TURN response integrity")
    return kind, result


def xor_address(address, port):
    return struct.pack("!BBHI", 0, 1, port ^ (MAGIC >> 16), int.from_bytes(socket.inet_aton(address), "big") ^ MAGIC)


def decoded_address(value):
    if len(value) != 8 or value[:2] != b"\0\1":
        raise AssertionError("Expected IPv4 TURN address")
    _, _, port, ip = struct.unpack("!BBHI", value)
    return socket.inet_ntoa((ip ^ MAGIC).to_bytes(4, "big")), port ^ (MAGIC >> 16)


class Client:
    def __init__(self, port, tcp=False):
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM if tcp else socket.SOCK_DGRAM)
        self.socket.settimeout(3)
        self.socket.connect(("127.0.0.1", port))
        self.tcp = tcp
        self.auth = []
        self.key = None

    def receive(self):
        if not self.tcp:
            return self.socket.recv(65535)
        def exact(size):
            value = b""
            while len(value) < size:
                block = self.socket.recv(size - len(value))
                if not block: raise AssertionError("TURN TCP connection closed")
                value += block
            return value
        header = exact(20)
        return header + exact(struct.unpack("!H", header[2:4])[0])

    def exchange(self, method, attributes, authenticated=False):
        data, transaction = packet(method, attributes + (self.auth if authenticated else []), self.key if authenticated else None)
        self.socket.sendall(data)
        response = self.receive()
        kind, result = parse(response, transaction)
        if authenticated and kind == 0x0100 | method:
            parse(response, transaction, self.key)
        return kind, result

    def allocate(self, username, password):
        attributes = [(0x0019, b"\x11\0\0\0"), (0x000d, struct.pack("!I", 60))]
        kind, challenge = self.exchange(3, attributes)
        assert kind == 0x0113 and challenge[0x0009][2:4] == b"\x04\x01", "TURN did not require authentication"
        realm, nonce = challenge[0x0014], challenge[0x0015]
        self.auth = [(0x0006, username.encode()), (0x0014, realm), (0x0015, nonce)]
        self.key = hashlib.md5(username.encode() + b":" + realm + b":" + password.encode()).digest()
        return self.exchange(3, attributes, True)

    def close(self, release=False):
        try:
            if release:
                kind, _ = self.exchange(4, [(0x000d, b"\0\0\0\0")], True)
                assert kind == 0x0104, "TURN allocation did not release"
        finally:
            self.socket.close()


def credentials(secret, expiry):
    username = f"{expiry}:loopback-test"
    return username, base64.b64encode(hmac.new(secret.encode(), username.encode(), hashlib.sha1).digest()).decode()


def listener_port():
    with socket.socket() as tcp:
        tcp.bind(("127.0.0.1", 0))
        port = tcp.getsockname()[1]
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp:
            udp.bind(("127.0.0.1", port))
    return port


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--turnserver", help="Explicit pinned test binary; default is the installed coturn")
    parser.add_argument("--media", action="store_true", help="Also test real GStreamer decoded media/input over a forced loopback relay")
    args = parser.parse_args()
    binary = str(Path(args.turnserver).resolve(strict=True)) if args.turnserver else shutil.which("turnserver")
    if not binary or os.name != "posix":
        raise SystemExit("Run on Linux with coturn installed; no sudo required")
    with tempfile.TemporaryDirectory(prefix="4play-turn-loopback-") as directory:
        root = Path(directory)
        root.chmod(0o700)
        port = listener_port()
        # This fixture range is disjoint from production ICE/relay pools.
        relay_start, relay_end = 62000, 62031
        held = []
        try:
            for relay_port in range(relay_start, relay_end + 1):
                sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                sock.bind(("127.0.0.1", relay_port))
                held.append(sock)
        finally:
            for sock in held: sock.close()
        secret = secrets.token_hex(32)
        config = root / "turn.conf"
        config.write_text(f"""listening-ip=127.0.0.1
relay-ip=127.0.0.1
listening-port={port}
min-port={relay_start}
max-port={relay_end}
realm=4play-loopback-test.invalid
use-auth-secret
static-auth-secret={secret}
userdb={root}/turn.db
no-auth-pings
no-tls
no-dtls
no-cli
no-tcp-relay
no-multicast-peers
no-rfc5780
allow-loopback-peers
cli-password={secrets.token_hex(32)}
relay-threads=1
user-quota=4
total-quota=8
max-bps=1000000
pidfile={root}/turn.pid
log-file={root}/turn.log
simple-log
no-stdout-log
""", encoding="utf-8")
        config.chmod(0o600)
        process = subprocess.Popen([binary, "-c", str(config)], cwd=root, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        echo = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        echo.bind(("127.0.0.1", 0))
        echo.settimeout(.1)
        stop = threading.Event()
        sources = []

        def peer():
            while not stop.is_set():
                try: data, source = echo.recvfrom(65535)
                except socket.timeout: continue
                sources.append(source)
                echo.sendto(data, source)

        thread = threading.Thread(target=peer, daemon=True)
        thread.start()
        try:
            deadline = time.monotonic() + 5
            while True:
                if process.poll() is not None: raise AssertionError("Isolated coturn failed to start")
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=.2): break
                except OSError:
                    if time.monotonic() >= deadline: raise AssertionError("Isolated coturn startup timeout")
                    time.sleep(.05)
            for tcp in [False, True]:
                client = Client(port, tcp)
                allocated = False
                try:
                    kind, result = client.allocate(*credentials(secret, int(time.time()) + 120))
                    assert kind == 0x0103, "Valid REST credential rejected"
                    allocated = True
                    relay_ip, relay_port = decoded_address(result[0x0016])
                    assert relay_ip == "127.0.0.1" and relay_start <= relay_port <= relay_end
                    kind, _ = client.exchange(8, [(0x0012, xor_address("127.0.0.1", echo.getsockname()[1]))], True)
                    assert kind == 0x0108, "TURN permission rejected"
                    for i in range(3):
                        payload = f"4play-relay-test-{i}".encode()
                        data, _ = packet(0x0016, [(0x0012, xor_address("127.0.0.1", echo.getsockname()[1])), (0x0013, payload)])
                        client.socket.sendall(data)
                        kind, response = parse(client.receive())
                        assert kind == 0x0017 and response[0x0013] == payload, "Relay payload mismatch"
                    assert sources[-1] == (relay_ip, relay_port), "Peer did not receive traffic from the relay allocation"
                finally:
                    client.close(allocated)
                print(f"PASS: REST-authenticated {'TCP' if tcp else 'UDP'} TURN client, bounded UDP relay, payload round trip, release")
            for label, credential in [
                ("expired", credentials(secret, int(time.time()) - 60)),
                ("wrong-secret", credentials(secrets.token_hex(32), int(time.time()) + 120))]:
                client = Client(port)
                try:
                    kind, response = client.allocate(*credential)
                    assert kind == 0x0113 and response[0x0009][2:4] == b"\x04\x01", "Invalid REST credential accepted"
                finally: client.close()
                print(f"PASS: {label} credential denied")
            if args.media:
                from webrtc_turn_fixture import test_relay_media
                test_relay_media(port, secret, root, process.pid)
        finally:
            stop.set()
            thread.join(timeout=2)
            echo.close()
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=3)
    print("PASS: temporary coturn stopped; no public service, firewall, router, or production secret changed")


if __name__ == "__main__": main()
