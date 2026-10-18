"""Fail-closed staging configuration and filesystem rollback tests; no root/network."""
import importlib.util
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "deploy/nginx/prepare-public-edge.py"
spec = importlib.util.spec_from_file_location("public_edge", SOURCE)
edge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(edge)


class EdgeTests(unittest.TestCase):
    def test_http_config_with_real_nginx_when_available(self):
        nginx = shutil.which("nginx")
        if not nginx and Path("/usr/sbin/nginx").is_file():
            nginx = "/usr/sbin/nginx"
        if not nginx:
            self.skipTest("nginx parser unavailable on this host")
        # nginx -t also checks listener binding on this build. Use an unprivileged
        # loopback port so validation neither needs root nor touches live port 80.
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        rendered = edge.render().replace("listen 80;", f"listen 127.0.0.1:{port};")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / "nginx.conf"
            config.write_text(
                f"error_log stderr;\npid {root.as_posix()}/nginx.pid;\n"
                "events {}\nhttp {\naccess_log off;\n" + rendered + "}\n",
                encoding="utf-8")
            result = subprocess.run([nginx, "-e", "stderr", "-t", "-p", str(root), "-c", str(config)],
                                    capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_staging_never_proxies_or_enables_application(self):
        for tls in [False, True]:
            config = edge.render(tls)
            self.assertNotIn("proxy_pass", config)
            self.assertNotIn("default_server", config)
            self.assertNotIn("listen [::]", config)
            self.assertIn("return 503", config)
            self.assertIn("location ^~ /.well-known/acme-challenge/", config)
            self.assertIn("try_files $uri =404", config)
            self.assertIn("Cache-Control", config)
            self.assertIn("root /var/www/4play-acme;", config)
            self.assertEqual("listen 443 ssl" in config, tls)
            self.assertEqual("ssl_certificate_key" in config, tls)

    def exercise(self, fail_at=None, previous=None):
        if os.name == "nt":
            self.skipTest("nginx installer filesystem tests require Linux symlink semantics")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            site, enabled = root / "site", root / "enabled"
            if previous:
                site.write_text(previous, encoding="utf-8")
                enabled.symlink_to(site)
            calls = []

            def run(args, **_kwargs):
                calls.append(args)
                if len(calls) == fail_at:
                    raise subprocess.CalledProcessError(1, args)

            if fail_at:
                with self.assertRaises(subprocess.CalledProcessError):
                    edge.install(edge.render(True), site, enabled, run)
                self.assertEqual(site.read_text() if site.exists() else None, previous)
                self.assertEqual(enabled.is_symlink(), previous is not None)
                if fail_at > 1:
                    self.assertEqual(calls[-1], ["systemctl", "reload", "nginx"])
            else:
                edge.install(edge.render(True), site, enabled, run)
                self.assertEqual(site.read_text(), edge.render(True))
                self.assertEqual(enabled.readlink(), site)
                self.assertEqual(calls, [["nginx", "-t"], ["nginx", "-t"],
                                         ["systemctl", "reload", "nginx"]])

    def test_install(self):
        self.exercise()

    def test_upgrade(self):
        self.exercise(previous=edge.render(False))

    def test_validation_and_reload_failures_restore_previous_config(self):
        for fail_at in [1, 2, 3]:
            for previous in [None, edge.render(False)]:
                with self.subTest(fail_at=fail_at, previous=bool(previous)):
                    self.exercise(fail_at, previous)

    def test_unknown_config_is_untouched(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            site = root / "site"
            site.write_text("unrelated site", encoding="utf-8")
            with self.assertRaises(ValueError):
                edge.install(edge.render(), site, root / "enabled")
            self.assertEqual(site.read_text(), "unrelated site")

    def test_wrong_link_is_untouched(self):
        if os.name == "nt":
            self.skipTest("nginx installer filesystem tests require Linux symlink semantics")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            enabled = root / "enabled"
            enabled.symlink_to(root / "other")
            with self.assertRaises(ValueError):
                edge.install(edge.render(), root / "site", enabled)
            self.assertEqual(enabled.readlink(), root / "other")


if __name__ == "__main__":
    unittest.main()
