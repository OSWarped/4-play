"""Isolated nginx TLS routing/header tests. Never modifies or reloads live nginx."""
import http.server
import json
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

SOURCE = Path(__file__).resolve().parents[1] / "deploy/nginx/arcade-public.conf.example"


class NginxTests(unittest.TestCase):
    def test_candidate_never_proxies_unscoped_or_admin_routes(self):
        config = SOURCE.read_text(encoding="utf-8")
        self.assertNotIn("location / { proxy_pass", config)
        self.assertIn("location / { return 404; }", config)
        self.assertNotIn("$request ", config)
        self.assertNotIn("$request_uri $status", config)
        self.assertIn('proxy_set_header Authorization "";', config)

    def test_live_isolated_tls_route_allowlist_and_edge_headers(self):
        nginx = shutil.which("nginx") or ("/usr/sbin/nginx" if Path("/usr/sbin/nginx").is_file() else None)
        openssl = shutil.which("openssl")
        if not nginx or not openssl:
            self.skipTest("Linux nginx/openssl required")

        class Backend(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                encoded = json.dumps(dict(self.headers)).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(encoded)

            def log_message(self, *_args): pass

        def port():
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 0))
                return listener.getsockname()[1]

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            backend = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Backend)
            thread = threading.Thread(target=backend.serve_forever, daemon=True)
            thread.start()
            process = None
            try:
                subprocess.run([openssl, "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                    "-subj", "/CN=localhost", "-keyout", str(root / "key.pem"), "-out", str(root / "cert.pem")],
                    check=True, capture_output=True, timeout=20)
                snippet = root / "edge.conf"
                snippet.write_text('proxy_set_header X-4Play-Edge-Key "test-edge-key";\n', encoding="utf-8")
                tls_port = port()
                text = SOURCE.read_text(encoding="utf-8").replace("listen 80;", f"listen 127.0.0.1:{port()};")
                text = text.replace("listen 443 ssl;", f"listen 127.0.0.1:{tls_port} ssl;")
                text = text.replace("/etc/letsencrypt/live/arcade.blakemilam.com/fullchain.pem", (root/"cert.pem").as_posix())
                text = text.replace("/etc/letsencrypt/live/arcade.blakemilam.com/privkey.pem", (root/"key.pem").as_posix())
                text = text.replace("/etc/nginx/snippets/4play-wan-edge-key.conf", snippet.as_posix())
                text = text.replace("/var/log/nginx/4play-wan-access.log", (root/"access.log").as_posix())
                text = text.replace("/var/log/nginx/4play-wan-error.log", (root/"error.log").as_posix())
                text = text.replace("http://127.0.0.1:8080", f"http://127.0.0.1:{backend.server_port}")
                config = root / "nginx.conf"
                config.write_text(f"daemon off;\nerror_log stderr;\npid {root.as_posix()}/nginx.pid;\nevents {{}}\nhttp {{\naccess_log off;\n{text}\n}}\n", encoding="utf-8")
                process = subprocess.Popen([nginx, "-e", "stderr", "-p", str(root), "-c", str(config)], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
                # Only this throwaway self-signed fixture disables certificate validation.
                context = ssl._create_unverified_context()
                deadline = time.monotonic() + 8
                while True:
                    try:
                        with socket.create_connection(("127.0.0.1", tls_port), timeout=.2): break
                    except OSError:
                        if time.monotonic() >= deadline: self.fail("Isolated nginx failed to start")
                        time.sleep(.05)
                for path in ["/admin", "/api/v1/games", "/client", "/client-v4/unknown", "/sessions/123/offer"]:
                    with self.assertRaises(urllib.error.HTTPError) as error:
                        urllib.request.urlopen(f"https://127.0.0.1:{tls_port}{path}", context=context, timeout=3)
                    self.assertEqual(error.exception.code, 404)
                for path in ["/client-v4", "/client/app.js", "/client-v2/tester-access", "/client-v2/api/games"]:
                    request = urllib.request.Request(f"https://127.0.0.1:{tls_port}{path}", headers={
                        "Authorization":"Bearer should-not-pass", "X-4Play-Edge-Key":"forged", "X-4Play-Client-IP":"8.8.8.8",
                        "Forwarded":"for=8.8.8.8", "X-Forwarded-For":"8.8.8.8", "X-4Play-Media-Profile":"wan"})
                    with urllib.request.urlopen(request, context=context, timeout=3) as response:
                        headers = {k.lower():v for k,v in json.load(response).items()}
                    self.assertEqual(headers["host"], "arcade.blakemilam.com")
                    self.assertEqual(headers["x-4play-edge-key"], "test-edge-key")
                    self.assertEqual(headers["x-4play-client-ip"], "127.0.0.1")
                    for name in ["authorization", "forwarded", "x-forwarded-for", "x-4play-media-profile"]:
                        self.assertNotIn(name, headers)
            finally:
                if process:
                    process.terminate()
                    process.communicate(timeout=8)
                backend.shutdown()
                backend.server_close()
                thread.join(timeout=3)


if __name__ == "__main__": unittest.main()
