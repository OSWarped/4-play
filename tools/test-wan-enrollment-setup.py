"""Private enrollment templates and rollback checks; no live configuration writes."""
import contextlib
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

directory = Path(__file__).resolve().parents[1] / "deploy/systemd"
sys.path.insert(0, str(directory))
spec = importlib.util.spec_from_file_location("enrollment_setup", directory / "configure-wan-enrollment.py")
setup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(setup)
KEYS = ("a"*64, "b"*64, "c"*64)


class EnrollmentTests(unittest.TestCase):
    def test_separate_keys_same_database_and_ordered_token_overrides(self):
        files = setup.contents(*KEYS)
        self.assertEqual(len(files), 6)
        tokens = files[setup.SETTINGS / "service-tokens.env"][0].decode()
        self.assertIn("FOURPLAY_SEAT_API_TOKEN=" + KEYS[0], tokens)
        self.assertIn("FOURPLAY_RUNTIME_HOST_API_TOKEN=" + KEYS[1], tokens)
        cp = files[setup.SETTINGS / "control-plane.env"][0].decode()
        self.assertIn("FOURPLAY_WAN_TESTER_ORIGIN=https://arcade.blakemilam.com\n", cp)
        self.assertIn("FOURPLAY_WAN_EDGE_KEY=" + KEYS[2], cp)
        for name in ("control-plane.env", "runtime-host.env"):
            self.assertIn("FOURPLAY_WAN_TESTER_DATABASE=" + str(setup.DATABASE), files[setup.SETTINGS / name][0].decode())
        for service in setup.delivery.SERVICES:
            definition, mode = files[setup.UNITS / (service + ".d") / setup.DROPIN]
            self.assertEqual(mode, 0o644)
            self.assertTrue(all(key.encode() not in definition for key in KEYS))
            self.assertIn(b"service-tokens.env", definition)
        self.assertEqual(files[setup.SNIPPET], (f'proxy_set_header X-4Play-Edge-Key "{KEYS[2]}";\n'.encode(), 0o600))
        self.assertNotIn(KEYS[2].encode(), files[setup.SETTINGS / "runtime-host.env"][0])

    def test_invalid_duplicate_and_injectable_keys_are_rejected(self):
        for keys in ((KEYS[0], KEYS[0], KEYS[2]), ("short", *KEYS[1:]), ("\n" + KEYS[0], *KEYS[1:])):
            with self.assertRaises(ValueError): setup.contents(*keys)

    @contextlib.contextmanager
    def fixture(self):
        with tempfile.TemporaryDirectory() as root, contextlib.ExitStack() as stack:
            base = Path(root)
            (base / "units").mkdir(); (base / "snippets").mkdir()
            for name, value in (("SETTINGS", base / "settings"), ("UNITS", base / "units"), ("SNIPPET", base / "snippets/key.conf")):
                stack.enter_context(patch.object(setup, name, value))
            stack.enter_context(patch.object(setup.delivery.stage, "check_metadata"))
            stack.enter_context(patch.object(setup.delivery.os, "fchmod", create=True))
            run = stack.enter_context(patch.object(setup.delivery, "run"))
            yield base, run

    def test_install_is_idempotent_retains_keys_and_never_restarts_or_reloads_nginx(self):
        with self.fixture() as (_, run):
            files = setup.contents(*KEYS)
            self.assertEqual(setup.install(files), 6)
            self.assertEqual(setup.install(files), 0)
            with patch.object(setup.delivery.stage, "protected_ancestors"):
                self.assertEqual(setup.material(), KEYS)
            commands = [call.args[0] for call in run.call_args_list]
            self.assertTrue(all(cmd == ["/usr/bin/systemctl", "daemon-reload"] or cmd[:2] == ["/usr/bin/systemd-analyze", "verify"] for cmd in commands))
            for path, (data, _) in files.items(): self.assertEqual(path.read_bytes(), data)

    def test_reload_failure_restores_previous_configuration_and_preserves_unrelated_files(self):
        with self.fixture() as (base, run):
            marker = base / "units/keep"; marker.write_bytes(b"untouched")
            run.side_effect = [RuntimeError("validation failed"), None]
            with self.assertRaises(RuntimeError): setup.install(setup.contents(*KEYS))
            self.assertFalse(setup.SETTINGS.exists())
            self.assertFalse(setup.SNIPPET.exists())
            self.assertEqual(list((base / "units").iterdir()), [marker])

    def test_conflicting_snippet_refuses_every_write(self):
        with self.fixture() as (_, run):
            setup.SNIPPET.write_bytes(b"existing operator snippet")
            with self.assertRaises(ValueError): setup.install(setup.contents(*KEYS))
            self.assertFalse(setup.SETTINGS.exists())
            self.assertEqual(setup.SNIPPET.read_bytes(), b"existing operator snippet")
            run.assert_not_called()

    def test_generation_is_distinct_and_partial_settings_fail_closed(self):
        with self.fixture():
            values = setup.material()
            self.assertEqual(len(set(values)), 3)
            setup.contents(*values)
            setup.SETTINGS.mkdir()
            (setup.SETTINGS / "service-tokens.env").write_bytes(b"partial")
            with self.assertRaises(ValueError): setup.material()

    def test_describe_is_unprivileged_and_does_not_read_keys_or_touch_host(self):
        with patch.object(sys, "argv", ["setup", "--describe"]), patch.object(setup, "verify_host") as verify, patch.object(
            setup, "material"
        ) as material, patch.object(setup, "install") as install, contextlib.redirect_stdout(io.StringIO()) as output:
            setup.main()
            verify.assert_not_called(); material.assert_not_called(); install.assert_not_called()
            self.assertTrue(all(key not in output.getvalue() for key in KEYS))

    @unittest.skipUnless(sys.platform == "linux", "Linux root flow with privileged checks mocked")
    def test_host_preflight_failure_prevents_key_generation_and_installation(self):
        with patch.object(sys, "argv", ["setup", "--apply"]), patch.object(setup.os, "geteuid", return_value=0), patch.object(
            setup, "verify_host", side_effect=ValueError("fixture")
        ), patch.object(setup, "material") as material, patch.object(setup, "install") as install:
            with self.assertRaises(ValueError): setup.main()
            material.assert_not_called(); install.assert_not_called()


if __name__ == "__main__": unittest.main()
