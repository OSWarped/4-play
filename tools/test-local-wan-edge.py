"""Guarded local edge checker tests, with synthetic keys and disposable services."""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("local_wan_edge_check", ROOT / "tools/check-local-wan-edge.py")
edge = importlib.util.module_from_spec(spec); spec.loader.exec_module(edge)


class EdgeTests(unittest.TestCase):
    def test_render_is_loopback_only_and_preserves_production_security_routes(self):
        text = edge.render(Path("/run/fixture"), 18080, 18443)
        self.assertIn("listen 127.0.0.1:18080;", text)
        self.assertIn("listen 127.0.0.1:18443 ssl;", text)
        self.assertNotIn("listen 80;", text); self.assertNotIn("listen 443 ssl;", text)
        self.assertIn("proxy_set_header X-4Play-Client-IP $remote_addr;", text)
        self.assertIn('proxy_set_header Authorization "";', text)
        self.assertIn("location / { return 404; }", text)
        self.assertIn("include /etc/nginx/snippets/4play-wan-edge-key.conf;", text)
        self.assertNotIn("/var/log/nginx/", text)

    def test_plan_never_opens_credentials_or_starts_services(self):
        with patch.object(sys, "argv", ["edge"]), patch.object(edge, "run") as run, patch.object(edge, "fixture") as fixture, contextlib.redirect_stdout(io.StringIO()):
            edge.main(); run.assert_not_called(); fixture.assert_not_called()

    def test_status_errors_do_not_expose_response_contents(self):
        with self.assertRaises(RuntimeError) as error:
            edge.require((500, {}, b"private-value"), 200)
        self.assertNotIn("private-value", str(error.exception))

    def test_failed_login_still_revokes_only_its_own_fixture_invitation(self):
        identifier = "2a90cae0-34ba-4944-8ea1-af8b68106ebd"
        def command(args):
            return SimpleNamespace(stdout=f"Invitation ID: {identifier}\nInvitation code (shown once): private-value\n")
        def request(_port, path, **kwargs):
            if path == "/client-v2/tester-access" and kwargs.get("method") == "POST":
                raise RuntimeError("fixture interruption")
            if kwargs.get("direct"): return 403, {}, b'{"code":"tester_edge_denied"}'
            if path in ("/admin", "/api/v1/games", "/api/v1/sessions", "/client", "/sessions/123/offer", "/client-v4/unknown"):
                return 404, {}, b""
            if path in ("/client-v2/bootstrap", "/client-v2/api/games", "/client-v2/webrtc-config"):
                return 401, {}, b'{"code":"tester_access_required"}'
            return 200, {}, b""
        with patch.object(edge, "request", side_effect=request), patch.object(edge, "run", side_effect=command) as run, contextlib.redirect_stdout(io.StringIO()) as output:
            with self.assertRaisesRegex(RuntimeError, "fixture interruption"):
                edge.checks(18443, SimpleNamespace(DATABASE=Path("private.sqlite")))
            self.assertEqual(len(run.call_args_list), 2)
            self.assertEqual(run.call_args_list[-1].args[0][-2:], ["--id", identifier])
            self.assertIn("revoke", run.call_args_list[-1].args[0])
            self.assertNotIn("private-value", output.getvalue())

    @unittest.skipUnless(sys.platform == "linux" and Path("/usr/sbin/nginx").is_file() and
                         (ROOT / "target/release/4play-tester-access").is_file() and
                         (ROOT / "target/release/control-plane-server").is_file(), "Linux nginx and release binaries required")
    def test_real_disposable_nginx_rust_login_catalog_and_revocation(self):
        with socket.socket() as test:
            try: test.bind((edge.SOURCE_IP, 0))
            except OSError: self.skipTest("Reference host LAN source address unavailable")
        with tempfile.TemporaryDirectory(prefix="4play-edge-regression-") as directory:
            root = Path(directory)
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 0)); backend_port = listener.getsockname()[1]
            subprocess.run([shutil.which("openssl"), "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                "-subj", "/CN=" + edge.DOMAIN, "-addext", "subjectAltName=DNS:" + edge.DOMAIN,
                "-keyout", str(root / "key.pem"), "-out", str(root / "cert.pem")], check=True, capture_output=True, timeout=20)
            snippet = root / "edge.conf"
            snippet.write_text('proxy_set_header X-4Play-Edge-Key "' + "c"*64 + '";\n')
            text = edge.SOURCE.read_text().replace("/etc/nginx/snippets/4play-wan-edge-key.conf", snippet.as_posix())
            text = text.replace("/etc/letsencrypt/live/arcade.blakemilam.com/fullchain.pem", (root / "cert.pem").as_posix())
            text = text.replace("/etc/letsencrypt/live/arcade.blakemilam.com/privkey.pem", (root / "key.pem").as_posix())
            text = text.replace("http://127.0.0.1:8080", f"http://127.0.0.1:{backend_port}")
            candidate = root / "candidate.conf"; candidate.write_text(text)
            database = root / "invites.sqlite"
            database.touch(mode=0o600)
            settings = {key: value for key, value in os.environ.items() if not key.startswith("FOURPLAY_")}
            settings.update(FOURPLAY_CONTROL_PLANE_BIND=f"127.0.0.1:{backend_port}",
                FOURPLAY_CONTROL_PLANE_DATABASE=str(root / "control.sqlite"), FOURPLAY_SEAT_API_TOKEN="a"*64,
                FOURPLAY_RUNTIME_HOST_API_TOKEN="b"*64, FOURPLAY_WAN_TESTER_ORIGIN=edge.ORIGIN,
                FOURPLAY_WAN_EDGE_KEY="c"*64, FOURPLAY_WAN_TESTER_DATABASE=str(database),
                FOURPLAY_WAN_TURN_HOST=edge.DOMAIN, FOURPLAY_WAN_TURN_SECRET="d"*64)
            process = subprocess.Popen([str(ROOT / "target/release/control-plane-server")], env=settings,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                deadline = time.monotonic() + 5
                while True:
                    if process.poll() is not None: self.fail("Disposable backend exited")
                    try:
                        with socket.create_connection(("127.0.0.1", backend_port), timeout=.2): break
                    except OSError:
                        if time.monotonic() >= deadline: self.fail("Disposable backend not ready")
                        time.sleep(.05)
                original_run = edge.run
                def unprivileged_run(args):
                    if args[0] == "/usr/sbin/runuser": args = args[args.index("--") + 1:]
                    return original_run(args)
                context = ssl.create_default_context(cafile=str(root / "cert.pem"))
                with patch.object(edge, "SOURCE", candidate), patch.object(edge, "BACKEND_PORT", backend_port), patch.object(
                    edge.ssl, "create_default_context", return_value=context
                ), patch.object(edge, "run", side_effect=unprivileged_run), contextlib.redirect_stdout(io.StringIO()) as output:
                    with edge.fixture(root) as port:
                        edge.checks(port, SimpleNamespace(DATABASE=database))
                for private in ("a"*64, "b"*64, "c"*64, "d"*64):
                    self.assertNotIn(private, output.getvalue())
                self.assertIn("revocation immediately rejects", output.getvalue())
            finally:
                process.terminate()
                try: process.wait(timeout=5)
                except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)


if __name__ == "__main__": unittest.main()
