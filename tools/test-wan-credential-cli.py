"""Exercise the real Rust/Python no-listener diagnostic CLIs with temp secrets."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import struct

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/control-plane-server"


@unittest.skipUnless(sys.platform == "linux" and BINARY.is_file(), "Build release control-plane-server on Linux first")
class DiagnosticTests(unittest.TestCase):
    def check(self, content, *, conflict=False, extra=False, acl=None):
        with tempfile.TemporaryDirectory(prefix="4play-credential-cli-") as temporary:
            directory = Path(temporary)
            credential = directory / "4play-turn.conf"
            credential.write_text(content); credential.chmod(0o600)
            if acl is not None:
                entries = [(1, 4, 0xffffffff), (2, 4, os.geteuid()), (4, 0, 0xffffffff),
                           (16, 4, 0xffffffff), (32, 0, 0xffffffff)]
                if acl == "group": entries[2] = (4, 4, 0xffffffff)
                if acl == "other-user": entries[1] = (2, 4, os.geteuid() + 1)
                if acl == "writable": entries[1] = (2, 6, os.geteuid()); entries[3] = (16, 6, 0xffffffff)
                os.setxattr(credential, "system.posix_acl_access", struct.pack("<I", 2) + b"".join(
                    struct.pack("<HHI", *entry) for entry in entries))
            settings = {key: value for key, value in os.environ.items() if key not in (
                "FOURPLAY_WAN_TURN_SECRET", "FOURPLAY_SEAT_API_TOKEN", "FOURPLAY_RUNTIME_HOST_API_TOKEN")}
            settings.update(FOURPLAY_WAN_TURN_CREDENTIAL="4play-turn.conf", CREDENTIALS_DIRECTORY=str(directory),
                            FOURPLAY_WAN_TURN_HOST="192.168.20.68", FOURPLAY_WAN_PUBLIC_IP="45.24.46.155",
                            FOURPLAY_CONTROL_PLANE_DATABASE=str(directory / "must-not-create" / "db.sqlite"))
            if conflict: settings["FOURPLAY_WAN_TURN_SECRET"] = "b"*64
            commands = [[str(BINARY), "--check-wan-turn-credential"]]
            if extra: commands[0].append("--unexpected")
            else: commands.append([sys.executable, str(ROOT / "tools/check-wan-turn-credential.py")])
            for command in commands:
                result = subprocess.run(command, env=settings, capture_output=True, text=True, timeout=10)
                self.assertNotIn("a"*64, result.stdout + result.stderr)
                self.assertNotIn("b"*64, result.stdout + result.stderr)
                self.assertFalse((directory / "must-not-create").exists())
                expected = content.count("static-auth-secret=") == 1 and not conflict and not extra and acl in (None, "systemd")
                self.assertEqual(result.returncode == 0, expected, result.stderr)
                if expected: self.assertIn("PASS:", result.stdout)

    def test_valid_credential_works_without_api_tokens_listener_or_database(self):
        self.check("realm=test\nstatic-auth-secret=" + "a"*64 + "\n")

    def test_duplicate_and_missing_credentials_fail_without_side_effects_or_secret_output(self):
        self.check("realm=test\n")
        self.check("static-auth-secret=" + "a"*64 + "\nstatic-auth-secret=" + "b"*64 + "\n")

    def test_plain_environment_conflict_fails_in_both_real_clis(self):
        self.check("static-auth-secret=" + "a"*64 + "\n", conflict=True)

    def test_extra_diagnostic_arguments_cannot_fall_through_to_server_startup(self):
        self.check("static-auth-secret=" + "a"*64 + "\n", extra=True)

    def test_systemd_named_user_acl_passes_but_shared_or_writable_acl_fails(self):
        for acl in ("systemd", "group", "other-user", "writable"):
            with self.subTest(acl=acl):
                self.check("static-auth-secret=" + "a"*64 + "\n", acl=acl)

    def test_actual_user_systemd_loadcredential_delivery(self):
        manager = subprocess.run(["systemctl", "--user", "is-system-running"], capture_output=True, text=True, timeout=10)
        if manager.stdout.strip() not in ("running", "degraded"):
            self.skipTest("No running user systemd manager; root installer still checks real service identities")
        with tempfile.TemporaryDirectory(prefix="4play-systemd-credential-") as temporary:
            path = Path(temporary) / "synthetic.conf"
            path.write_text("static-auth-secret=" + "a"*64 + "\n"); path.chmod(0o600)
            for command in ([str(BINARY), "--check-wan-turn-credential"],
                            [sys.executable, str(ROOT / "tools/check-wan-turn-credential.py")]):
                result = subprocess.run(["systemd-run", "--user", "--quiet", "--wait", "--collect", "--pipe",
                    "--property=LoadCredential=4play-turn.conf:" + str(path),
                    "--property=UnsetEnvironment=FOURPLAY_WAN_TURN_SECRET",
                    "--setenv=FOURPLAY_WAN_TURN_CREDENTIAL=4play-turn.conf",
                    "--setenv=FOURPLAY_WAN_TURN_HOST=192.168.20.68",
                    "--setenv=FOURPLAY_WAN_PUBLIC_IP=45.24.46.155", "--", *command],
                    capture_output=True, text=True, timeout=20)
                self.assertNotIn("a"*64, result.stdout + result.stderr)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("PASS:", result.stdout)


if __name__ == "__main__": unittest.main()
