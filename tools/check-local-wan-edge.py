#!/usr/bin/env python3
"""Exercise the live invitation gate through temporary loopback-only HTTPS nginx.

No live nginx reload, firewall change, game creation or public activation. One
one-hour test invitation/admission is created, then revoked and retained for audit.
Invitation values, cookies, service/edge keys and derived TURN credentials are
never printed. Requires root for the existing private snippet and TLS key.
"""
import argparse
import contextlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import socket
import ssl
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
DOMAIN = "arcade.blakemilam.com"
ORIGIN = "https://" + DOMAIN
SOURCE_IP = "192.168.20.68"
BACKEND_PORT = 8080
SOURCE = ROOT / "deploy/nginx/arcade-public.conf.example"


def run(args):
    return subprocess.run(args, check=True, capture_output=True, text=True, timeout=30)


def render(root, http_port, tls_port):
    """Keep candidate routes/headers intact; change only isolated listen/log paths."""
    text = SOURCE.read_text(encoding="utf-8")
    text = text.replace("listen 80;", f"listen 127.0.0.1:{http_port};")
    text = text.replace("listen 443 ssl;", f"listen 127.0.0.1:{tls_port} ssl;")
    text = text.replace("/var/log/nginx/4play-wan-access.log", (root / "access.log").as_posix())
    text = text.replace("/var/log/nginx/4play-wan-error.log", (root / "error.log").as_posix())
    if "listen 80;" in text or "listen 443 ssl;" in text:
        raise RuntimeError("Failed to isolate fixture listeners")
    return (f"daemon off;\npid {root.as_posix()}/nginx.pid;\nerror_log {root.as_posix()}/master.log crit;\n"
            "events {}\nhttp {\naccess_log off;\nproxy_buffering off;\nproxy_request_buffering off;\n" + text + "\n}\n")


def request(port, path, *, method="GET", data=None, cookie=None, extra=None, direct=False):
    headers = {"Host": DOMAIN, "Origin": ORIGIN, "X-4Play-Client": "v2", "Sec-Fetch-Site": "same-origin"}
    if cookie: headers["Cookie"] = cookie
    if extra: headers.update(extra)
    body = None if data is None else json.dumps(data)
    if body is not None: headers["Content-Type"] = "application/json"
    if direct:
        connection = http.client.HTTPConnection("127.0.0.1", BACKEND_PORT, timeout=10)
    else:
        connection = http.client.HTTPSConnection(DOMAIN, port, context=ssl.create_default_context(), timeout=10)
        # TLS still verifies the REAL certificate for DOMAIN. Only TCP routing is
        # local; binding the real LAN address keeps nginx's client IP non-loopback.
        connection._create_connection = lambda *_args, **_kwargs: socket.create_connection(
            ("127.0.0.1", port), timeout=10, source_address=(SOURCE_IP, 0))
    try:
        connection.request(method, path, body=body, headers=headers)
        response = connection.getresponse()
        content = response.read(2*1024*1024 + 1)
        if len(content) > 2*1024*1024: raise RuntimeError("Oversized fixture response")
        return response.status, {key.lower(): value for key, value in response.getheaders()}, content
    finally:
        connection.close()


def require(result, status, code=None):
    if result[0] != status: raise RuntimeError("Unexpected HTTP status; response omitted to protect credentials")
    if code is not None and json.loads(result[2]).get("code") != code:
        raise RuntimeError("Unexpected authorization result")


@contextlib.contextmanager
def fixture(parent="/run"):
    with tempfile.TemporaryDirectory(prefix="4play-private-edge-", dir=parent) as directory:
        root = Path(directory)
        with contextlib.ExitStack() as reservations:
            sockets = [reservations.enter_context(socket.socket()) for _ in range(2)]
            for listener in sockets: listener.bind(("127.0.0.1", 0))
            http_port, tls_port = [listener.getsockname()[1] for listener in sockets]
        config = root / "nginx.conf"
        config.write_text(render(root, http_port, tls_port), encoding="utf-8")
        config.chmod(0o600)
        run(["/usr/sbin/nginx", "-t", "-p", str(root), "-c", str(config)])
        process = subprocess.Popen(["/usr/sbin/nginx", "-p", str(root), "-c", str(config)],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        try:
            deadline = time.monotonic() + 5
            while True:
                if process.poll() is not None: raise RuntimeError("Private nginx exited")
                try:
                    with socket.create_connection(("127.0.0.1", tls_port), timeout=.2): break
                except OSError:
                    if time.monotonic() >= deadline: raise RuntimeError("Private nginx did not start")
                    time.sleep(.05)
            yield tls_port
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try: process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL); process.wait(timeout=5)


def checks(port, setup):
    for path in ("/client-v4", "/client/app.js", "/client-v2/tester-access"):
        require(request(port, path), 200)
    for path in ("/admin", "/api/v1/games", "/api/v1/sessions", "/client", "/sessions/123/offer", "/client-v4/unknown"):
        require(request(port, path), 404)
    print("PASS: actual candidate serves client/login assets and blocks admin/raw API routes")
    require(request(port, "/client-v2/bootstrap", method="POST", data={}), 401, "tester_access_required")
    require(request(port, "/client-v2/api/games"), 401, "tester_access_required")
    require(request(port, "/client-v2/webrtc-config"), 401, "tester_access_required")
    forged = {"X-4Play-Edge-Key": "0"*64, "X-4Play-Client-IP": "8.8.8.8", "Forwarded": "for=8.8.8.8",
              "X-Forwarded-For": "8.8.8.8", "X-4Play-Media-Profile": "lan", "Authorization": "Bearer fixture-invalid"}
    require(request(port, "/client-v2/bootstrap", method="POST", data={}, extra=forged), 401, "tester_access_required")
    require(request(port, "/client-v2/bootstrap", method="POST", data={}, extra=forged, direct=True), 403, "tester_edge_denied")
    print("PASS: missing admissions and forged direct/proxied trust headers cannot bypass the gate")
    cli = ["/usr/sbin/runuser", "--user", "blake", "--", str(ROOT / "target/release/4play-tester-access")]
    database = ["--database", str(setup.DATABASE)]
    invite_id = None
    try:
        issued = run(cli + ["issue", *database, "--hours", "1", "--seats", "1"]).stdout
        invite_id = str(uuid.UUID(re.search(r"^Invitation ID: (\S+)$", issued, re.MULTILINE).group(1)))
        invitation = re.search(r"^Invitation code \(shown once\): (\S+)$", issued, re.MULTILINE).group(1)
        enrolled = request(port, "/client-v2/tester-access", method="POST", data={"invitation": invitation})
        require(enrolled, 200)
        full_cookie = enrolled[1].get("set-cookie", "")
        if not all(part in full_cookie for part in ("Path=/client-v2;", "HttpOnly;", "Secure;", "SameSite=Strict;", "Max-Age=")):
            raise RuntimeError("Missing secure cookie attributes")
        cookie = full_cookie.split(";", 1)[0]
        bootstrap = request(port, "/client-v2/bootstrap", method="POST", data={}, cookie=cookie, extra=forged)
        require(bootstrap, 200)
        if json.loads(bootstrap[2]).get("destination_address") != SOURCE_IP:
            raise RuntimeError("Trusted client address was not replaced by nginx")
        require(request(port, "/client-v2/api/games", cookie=cookie), 200)
        media = request(port, "/client-v2/webrtc-config", cookie=cookie)
        require(media, 200)
        if not json.loads(media[2]).get("iceServers"): raise RuntimeError("WAN ICE configuration missing")
        require(request(port, "/client-v2/api/runtime-hosts", method="POST", data={}, cookie=cookie), 403)
        require(request(port, "/client-v2/bootstrap", method="POST", data={}, cookie=cookie,
                        extra={"Origin": "https://untrusted.invalid"}), 403)
        print("PASS: real invitation login, secure cookie, scoped catalog/WAN ICE and origin/admin restrictions")
    finally:
        if invite_id is not None:
            run(cli + ["revoke", *database, "--id", invite_id])
            print("PASS: fixture invitation revoked; audit record retained")
    require(request(port, "/client-v2/api/games", cookie=cookie), 401, "tester_access_required")
    print("PASS: revocation immediately rejects subsequent scoped requests")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", action="store_true")
    args = parser.parse_args()
    if not args.run:
        print(__doc__)
        print("Plan only. sudo python3 tools/check-local-wan-edge.py --run executes the check.")
        return
    if sys.platform != "linux" or os.geteuid() != 0: parser.error("Run with sudo on the reference Linux server")
    sys.path.insert(0, str(ROOT / "deploy/systemd"))
    spec = importlib.util.spec_from_file_location("local_edge_setup", ROOT / "deploy/systemd/configure-wan-enrollment.py")
    setup = importlib.util.module_from_spec(spec); spec.loader.exec_module(setup)
    setup.verify_host()
    setup.delivery.stage.read_private(setup.SETTINGS / "service-tokens.env")
    for path, (data, mode) in setup.contents(*setup.material()).items():
        setup.delivery.stage.protected_ancestors(path.parent)
        setup.delivery.stage.check_metadata(path.lstat(), private=mode == 0o600)
        if path.read_bytes() != data: raise RuntimeError("Enrollment files differ from reviewed templates")
    with fixture() as port:
        checks(port, setup)
    setup.verify_host()
    print("PASS: private nginx stopped; public staging, live services and firewall unchanged; no game started")


if __name__ == "__main__":
    try:
        main()
    except (Exception, KeyboardInterrupt):
        raise SystemExit("Local WAN edge check failed. Details/response bodies suppressed to protect credentials; public staging was not modified.") from None
