"""Explicit WAN ICE profile. LAN bounded-host policy remains the default.

Coturn REST credentials use its specified HMAC-SHA1 protocol. These generated
credentials are delegated one-hour credentials, not the persistent shared key.
"""
import base64
from dataclasses import dataclass, field
import hashlib
import hmac
import ipaddress
import os
import re
import time
import uuid
from pathlib import Path
import stat
from urllib.parse import quote

RELAY_START = 52000
RELAY_END = 52199
CREDENTIAL_SECONDS = 3600


def credential_secret(settings):
    plain, name = settings.get("FOURPLAY_WAN_TURN_SECRET"), settings.get("FOURPLAY_WAN_TURN_CREDENTIAL")
    if plain is not None and name is not None:
        raise ValueError("Configure only one TURN secret source")
    if name is None:
        return plain or ""
    try:
        directory = Path(settings.get("CREDENTIALS_DIRECTORY", ""))
        if name != "4play-turn.conf" or not directory.is_absolute(): raise ValueError()
        path = directory / name
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink > 1 or metadata.st_size > 65536 or (os.name == "posix" and metadata.st_mode & 0o077): raise ValueError()
        with path.open("rb") as stream:
            opened = os.fstat(stream.fileno())
            if (opened.st_dev, opened.st_ino) != (metadata.st_dev, metadata.st_ino): raise ValueError()
            data = stream.read(65537)
        if len(data) > 65536: raise ValueError()
        values = [line.removeprefix("static-auth-secret=") for line in data.decode().splitlines() if line.startswith("static-auth-secret=")]
        if len(values) != 1 or not re.fullmatch(r"[a-fA-F0-9]{64}", values[0]): raise ValueError()
        return values[0]
    except (OSError, ValueError):
        raise ValueError("TURN systemd credential is missing, unsafe or invalid") from None


def request_is_wan(headers, peer_ip):
    profile = headers.get("x-4play-media-profile")
    if profile is None:
        return False
    if profile != "wan" or not ipaddress.ip_address(peer_ip).is_loopback:
        raise ValueError("untrusted media profile")
    return True


@dataclass(frozen=True)
class WanIce:
    host: str
    secret: str = field(repr=False)
    public_ip: str

    @classmethod
    def from_environment(cls, settings=None):
        settings = os.environ if settings is None else settings
        host = settings.get("FOURPLAY_WAN_TURN_HOST", "")
        secret = credential_secret(settings)
        public = settings.get("FOURPLAY_WAN_PUBLIC_IP", "")
        if (len(host) > 253 or not host or any(not re.fullmatch(r"[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?", part) for part in host.split("."))
                or not re.fullmatch(r"[a-fA-F0-9]{64}", secret)):
            raise ValueError("WAN media requires a TURN hostname and a random 64-hex shared secret")
        try:
            address = ipaddress.ip_address(public)
            if address.version != 4 or not address.is_global:
                raise ValueError()
        except ValueError:
            raise ValueError("WAN media requires a public IPv4 address") from None
        return cls(host, secret, str(address))

    def configure(self, webrtc, now=None):
        username = f"{int(time.time() if now is None else now) + CREDENTIAL_SECONDS}:publisher-{uuid.uuid4()}"
        password = base64.b64encode(hmac.new(self.secret.encode(), username.encode(), hashlib.sha1).digest()).decode()
        webrtc.set_property("stun-server", f"stun://{self.host}:3478")
        uri = f"turn://{quote(username, safe='')}:{quote(password, safe='')}@{self.host}:3478"
        if not webrtc.emit("add-turn-server", uri):
            raise RuntimeError("could not configure WAN TURN server")

    def validate_answer(self, answer, ports):
        found = False
        for line in answer.splitlines():
            if not line.startswith("a=candidate:"):
                continue
            fields = line.split()
            try:
                if len(fields) < 8 or fields[2].lower() != "udp" or fields[6] != "typ":
                    raise ValueError()
                address = ipaddress.ip_address(fields[4])
                port = int(fields[5])
                if not 1 <= port <= 65535 or address.is_loopback or address.is_unspecified or address.is_multicast:
                    raise ValueError()
                kind = fields[7]
                if kind == "host":
                    if not ports.start <= port <= ports.end:
                        raise ValueError()
                elif kind == "srflx":
                    if str(address) != self.public_ip or "rport" not in fields:
                        raise ValueError()
                    if not ports.start <= int(fields[fields.index("rport") + 1]) <= ports.end:
                        raise ValueError()
                elif kind == "relay":
                    if str(address) != self.public_ip or not RELAY_START <= port <= RELAY_END:
                        raise ValueError()
                    found = True
                else:
                    raise ValueError()
            except (ValueError, IndexError):
                raise RuntimeError("WAN ICE candidate violates the configured transport policy") from None
        if not found:
            raise RuntimeError("WAN ICE gathering did not produce a configured TURN relay candidate")
