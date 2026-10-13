"""Server-only authorization and UDP forwarding for browser controller states."""
from dataclasses import dataclass, field
import json
import socket
import struct
import threading
import time
import uuid


@dataclass(frozen=True)
class InputAuthorization:
    token: bytes = field(repr=False)
    port: int
    player: int
    peer_id: str
    expires_ms: int

    @classmethod
    def from_headers(cls, headers):
        if not headers.get("x-4play-input-token"):
            return None
        result = cls(
            uuid.UUID(headers["x-4play-input-token"]).bytes,
            int(headers["x-4play-input-port"]),
            int(headers["x-4play-input-player"]),
            str(uuid.UUID(headers["x-4play-input-peer"])),
            int(headers["x-4play-input-expires"]),
        )
        if not (1 <= result.port <= 65535 and 1 <= result.player <= 16):
            raise ValueError("invalid browser input authorization")
        if result.expires_ms <= time.time_ns() // 1_000_000:
            raise ValueError("expired browser input authorization")
        return result


class BrowserInputRelay:
    def __init__(self):
        self.lock = threading.Lock()
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(("127.0.0.1", 0))
        self.authorization = None
        self.sequence = 0
        self.browser_sequence = None
        self.rate_epoch = time.monotonic()
        self.rate_count = 0

    def authorize(self, authorization):
        with self.lock:
            self._neutralize()
            self.authorization = authorization
            self.browser_sequence = None

    def renew(self, authorization):
        with self.lock:
            old = self.authorization
            if old is None or authorization is None:
                return False
            if (old.peer_id, old.player, old.port, old.token) != (
                authorization.peer_id, authorization.player, authorization.port, authorization.token
            ):
                return False
            self.authorization = authorization
            return True

    def receive(self, message, peer_id):
        if not isinstance(message, str) or len(message) > 512:
            return False
        try:
            state = json.loads(message)
            values = [state[key] for key in ("sequence", "buttons", "axis_x", "axis_y")]
        except (ValueError, KeyError, TypeError):
            return False
        if any(type(value) is not int for value in values):
            return False
        sequence, buttons, x, y = values
        if not (0 <= sequence <= 0xffffffff and 0 <= buttons <= 255 and x in (-1, 0, 1) and y in (-1, 0, 1)):
            return False
        with self.lock:
            authorization = self.authorization
            if authorization is None or authorization.peer_id != peer_id:
                return False
            if authorization.expires_ms <= time.time_ns() // 1_000_000:
                self._neutralize()
                self.authorization = None
                return False
            if self.browser_sequence is not None:
                delta = (sequence - self.browser_sequence) & 0xffffffff
                if not (0 < delta < 0x80000000):
                    return False
            now = time.monotonic()
            if now - self.rate_epoch >= 1:
                self.rate_epoch, self.rate_count = now, 0
            if self.rate_count >= 250:
                return False
            self.rate_count += 1
            self.browser_sequence = sequence
            self._send(buttons, x, y)
            return True

    def _send(self, buttons, x, y):
        authorization = self.authorization
        if authorization is None:
            return
        self.sequence = (self.sequence + 1) & 0xffffffff
        packet = struct.pack("<4sB16sIHhhBB", b"4PLY", 2, authorization.token,
                             self.sequence, buttons, x * 32767, y * 32767, 0, authorization.player)
        self.socket.sendto(packet, ("127.0.0.1", authorization.port))

    def _neutralize(self):
        self._send(0, 0, 0)

    def neutralize(self, peer_id):
        with self.lock:
            if self.authorization is not None and self.authorization.peer_id == peer_id:
                self._neutralize()

    def revoke(self, peer_id):
        with self.lock:
            if self.authorization is not None and self.authorization.peer_id == peer_id:
                self._neutralize()
                self.authorization = None

    def close(self):
        with self.lock:
            self._neutralize()
            self.authorization = None
            self.socket.close()
