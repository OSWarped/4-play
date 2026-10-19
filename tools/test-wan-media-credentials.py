"""Credential delivery templates and transaction tests; no real privileged work."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

directory = Path(__file__).resolve().parents[1] / "deploy/systemd"
sys.path.insert(0, str(directory))
spec = importlib.util.spec_from_file_location("media_credential_install", directory / "install-wan-media-credentials.py")
delivery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(delivery)


class CredentialTests(unittest.TestCase):
    def test_templates_have_only_credential_references_and_different_turn_hosts(self):
        files = delivery.contents("192.168.20.68", "45.24.46.155")
        self.assertEqual(len(files), 4)
        definitions = [data.decode() for path, (data, mode) in files.items() if path.suffix == ".conf"]
        for text in definitions:
            self.assertIn("LoadCredential=4play-turn.conf:/etc/4play-turn-policy1/turnserver.conf", text)
            self.assertIn("UnsetEnvironment=FOURPLAY_WAN_TURN_SECRET", text)
            self.assertNotIn("static-auth-secret", text)
        environments = {path.name: data.decode() for path, (data, mode) in files.items() if path.suffix == ".env"}
        self.assertIn("FOURPLAY_WAN_TURN_HOST=arcade.blakemilam.com", environments["4play-control-plane.env"])
        self.assertIn("FOURPLAY_WAN_TURN_HOST=192.168.20.68", environments["4play-runtime-host-agent.env"])
        self.assertTrue(all("FOURPLAY_WAN_TURN_SECRET=" not in text for text in environments.values()))

    def test_transient_checks_have_no_secret_argument_and_are_unprivileged_and_bounded(self):
        for service in delivery.SERVICES:
            args = delivery.credential_command(service, "blake", "192.168.20.68", "45.24.46.155")
            self.assertIn("--property=User=blake", args)
            self.assertIn("--property=RuntimeMaxSec=15s", args)
            self.assertIn("--property=RestrictAddressFamilies=AF_UNIX", args)
            self.assertIn("--property=CapabilityBoundingSet=", args)
            self.assertIn("--property=LoadCredential=4play-turn.conf:/etc/4play-turn-policy1/turnserver.conf", args)
            self.assertTrue(all("static-auth-secret" not in arg for arg in args))
            if service == delivery.SERVICES[0]: self.assertEqual(args[-1], "--check-wan-turn-credential")
            else: self.assertTrue(args[-1].endswith("check-wan-turn-credential.py"))

    def test_transaction_is_idempotent_and_never_restarts_application_services(self):
        with tempfile.TemporaryDirectory() as root, patch.object(delivery, "SETTINGS", Path(root) / "settings"), patch.object(
            delivery, "UNITS", Path(root)
        ), patch.object(delivery.stage, "check_metadata"), patch.object(delivery.os, "fchmod", create=True), patch.object(delivery, "run") as run:
            files = delivery.contents("192.168.20.68", "45.24.46.155")
            self.assertEqual(delivery.install(files), 4)
            self.assertEqual(delivery.install(files), 0)
            self.assertTrue(all(call.args[0] == ["/usr/bin/systemctl", "daemon-reload"] or call.args[0][:2] == ["/usr/bin/systemd-analyze", "verify"] for call in run.call_args_list))
            for path, (data, mode) in files.items(): self.assertEqual(path.read_bytes(), data)

    def test_reload_failure_rolls_back_only_new_files_and_empty_created_directories(self):
        with tempfile.TemporaryDirectory() as root, patch.object(delivery, "SETTINGS", Path(root) / "settings"), patch.object(
            delivery, "UNITS", Path(root)
        ), patch.object(delivery.stage, "check_metadata"), patch.object(delivery.os, "fchmod", create=True), patch.object(
            delivery, "run", side_effect=[RuntimeError("reload failed"), None]
        ):
            marker = Path(root) / "keep"; marker.write_bytes(b"original")
            with self.assertRaisesRegex(RuntimeError, "reload failed"):
                delivery.install(delivery.contents("192.168.20.68", "45.24.46.155"))
            self.assertEqual(list(Path(root).iterdir()), [marker]); self.assertEqual(marker.read_bytes(), b"original")

    def test_conflict_prevents_all_writes(self):
        with tempfile.TemporaryDirectory() as root, patch.object(delivery, "SETTINGS", Path(root) / "settings"), patch.object(
            delivery, "UNITS", Path(root)
        ), patch.object(delivery.stage, "check_metadata"), patch.object(delivery, "run") as run:
            files = delivery.contents("192.168.20.68", "45.24.46.155")
            conflict = next(path for path in files if path.suffix == ".conf")
            conflict.parent.mkdir(); conflict.write_bytes(b"custom")
            with self.assertRaises(ValueError): delivery.install(files)
            self.assertFalse(delivery.SETTINGS.exists()); self.assertEqual(conflict.read_bytes(), b"custom")
            run.assert_not_called()

    def test_describe_does_not_access_credentials_or_services(self):
        with patch.object(sys, "argv", ["delivery", "--describe"]), patch.object(delivery.installer, "verify_bundle") as verify, patch.object(delivery, "run") as run, contextlib.redirect_stdout(io.StringIO()):
            delivery.main(); verify.assert_not_called(); run.assert_not_called()

    @unittest.skipUnless(sys.platform == "linux", "Linux root flow with privileged actions mocked")
    def test_failed_transient_credential_check_prevents_all_configuration_writes(self):
        import pwd
        def run(args, **kwargs):
            if "tables" in args:
                return SimpleNamespace(stdout=json.dumps({"nftables":[{"table":{"family":"inet","name":name}} for name in ("fourplay_turn", "fourplay_turn_local_only")]}))
            if args[0] == "/usr/bin/systemd-run": raise RuntimeError("credential fixture failed")
            return SimpleNamespace(stdout="active")
        with contextlib.ExitStack() as stack:
            def replace(target, name, **kwargs): return stack.enter_context(patch.object(target, name, **kwargs))
            replace(sys, "argv", new=["credentials", "--apply"])
            replace(delivery.os, "geteuid", return_value=0)
            replace(pwd, "getpwnam", return_value=SimpleNamespace(pw_uid=105))
            replace(delivery.installer, "verify_bundle")
            replace(delivery.stage, "read_private", return_value=b'{"local_ip":"192.168.20.68","public_ip":"45.24.46.155"}')
            replace(delivery, "users_and_units", return_value={delivery.SERVICES[0]: "blake"})
            replace(delivery, "preflight"); replace(delivery, "check_relay_secret")
            replace(delivery, "run", side_effect=run)
            write = replace(delivery, "install")
            with self.assertRaisesRegex(RuntimeError, "credential fixture failed"): delivery.main()
            write.assert_not_called()


if __name__ == "__main__": unittest.main()
