"""Inactive unit installer checks: temp files/mocks only, never real systemd."""
import contextlib
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

directory = Path(__file__).resolve().parents[1] / "deploy/systemd"
sys.path.insert(0, str(directory))
spec = importlib.util.spec_from_file_location("tested_turn_units", directory / "install-tested-turn-units.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class UnitTests(unittest.TestCase):
    files = {"4play-turn.service": b"reviewed relay", "4play-turn-egress.service": b"reviewed policy"}

    def test_install_and_retry_only_reload_without_start_or_enable(self):
        with tempfile.TemporaryDirectory() as root, patch.object(installer, "TARGET", Path(root)), patch.object(
            installer.stage, "protected_ancestors"
        ), patch.object(installer.stage, "check_metadata"), patch.object(installer, "check_inactive"), patch.object(
            installer.os, "fchmod", create=True
        ), patch.object(installer.subprocess, "run") as run:
            self.assertEqual(installer.install(self.files), 2)
            self.assertEqual(installer.install(self.files), 0)
            self.assertEqual(len(list(Path(root).iterdir())), 2)
            self.assertTrue(all(call.args[0] == ["/usr/bin/systemctl", "daemon-reload"] for call in run.call_args_list))
            for name, data in self.files.items(): self.assertEqual((Path(root) / name).read_bytes(), data)

    def test_conflict_is_checked_before_any_new_file_or_reload(self):
        with tempfile.TemporaryDirectory() as root, patch.object(installer, "TARGET", Path(root)), patch.object(
            installer.stage, "protected_ancestors"
        ), patch.object(installer.stage, "check_metadata"), patch.object(installer.subprocess, "run") as run:
            conflict = Path(root) / "4play-turn.service"
            conflict.write_bytes(b"unrelated")
            with self.assertRaisesRegex(ValueError, "Conflicting unit"): installer.install(self.files)
            self.assertEqual(list(Path(root).iterdir()), [conflict])
            self.assertEqual(conflict.read_bytes(), b"unrelated"); run.assert_not_called()

    def test_failed_reload_removes_only_new_units_and_keeps_matching_preexisting(self):
        with tempfile.TemporaryDirectory() as root, patch.object(installer, "TARGET", Path(root)), patch.object(
            installer.stage, "protected_ancestors"
        ), patch.object(installer.stage, "check_metadata"), patch.object(installer.os, "fchmod", create=True), patch.object(
            installer.subprocess, "run", side_effect=[installer.subprocess.CalledProcessError(1, ["reload"]), None]
        ):
            existing = Path(root) / "4play-turn.service"
            existing.write_bytes(self.files[existing.name])
            with self.assertRaises(installer.subprocess.CalledProcessError): installer.install(self.files)
            self.assertEqual(list(Path(root).iterdir()), [existing])
            self.assertEqual(existing.read_bytes(), self.files[existing.name])

    def test_active_enabled_or_unmasked_turn_is_rejected(self):
        good = []
        for name in (*installer.NAMES, "coturn.service"):
            good.extend([SimpleNamespace(returncode=3, stdout="inactive"), SimpleNamespace(returncode=1, stdout="masked" if name == "coturn.service" else "disabled")])
        with patch.object(installer.subprocess, "run", side_effect=good): installer.check_inactive()
        for index, changed in [(0, SimpleNamespace(returncode=0, stdout="active")),
                               (1, SimpleNamespace(returncode=0, stdout="enabled")),
                               (5, SimpleNamespace(returncode=1, stdout="disabled"))]:
            invalid = good.copy(); invalid[index] = changed
            with patch.object(installer.subprocess, "run", side_effect=invalid):
                with self.assertRaises(ValueError): installer.check_inactive()

    def test_existing_metadata_rejection_prevents_read_or_reload(self):
        with tempfile.TemporaryDirectory() as root, patch.object(installer, "TARGET", Path(root)), patch.object(
            installer.stage, "protected_ancestors"
        ), patch.object(installer.stage, "check_metadata", side_effect=ValueError("symlink")), patch.object(installer.subprocess, "run") as run:
            (Path(root) / "4play-turn.service").write_bytes(b"fixture")
            with self.assertRaises(ValueError): installer.install(self.files)
            run.assert_not_called()

    def test_bundle_tampering_is_rejected_even_when_files_are_present(self):
        provenance = installer.bundle.candidate.receipt("b"*64)
        baseline = {"local_ip": "192.168.20.68", "public_ip": "45.24.46.155"}
        import json
        with tempfile.TemporaryDirectory() as root, patch.object(installer.bundle, "DESTINATION", Path(root)), patch.object(
            installer.bundle, "verified_baseline", return_value=(baseline, "c"*64)
        ), patch.object(installer.bundle.candidate, "verify_installed"), patch.object(installer.stage, "protected_ancestors"), patch.object(
            installer.stage, "existing_secret", return_value="a"*64
        ):
            files = installer.bundle.contents(105, baseline["local_ip"], baseline["public_ip"], "a"*64, provenance, "c"*64)
            for name in files: (Path(root) / name).touch()
            def read(path):
                return json.dumps(provenance).encode() if path.name == "candidate-receipt.json" else files[path.name]
            with patch.object(installer.stage, "read_private", side_effect=read):
                expected = installer.verify_bundle(105)
                self.assertEqual(set(expected), set(installer.NAMES))
                for name in ("egress.nft", "deployment-receipt.json"):
                    original = files[name]; files[name] = b"tampered"
                    with self.assertRaisesRegex(ValueError, "differs"): installer.verify_bundle(105)
                    files[name] = original

    def test_describe_never_reads_root_bundle_or_runs_systemd(self):
        with patch.object(sys, "argv", ["install", "--describe"]), patch.object(installer, "verify_bundle") as verify, patch.object(
            installer.subprocess, "run"
        ) as run, contextlib.redirect_stdout(io.StringIO()):
            installer.main(); verify.assert_not_called(); run.assert_not_called()


if __name__ == "__main__": unittest.main()
