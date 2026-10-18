"""Portable staging/permission regression tests; never modify /etc or services."""
import importlib.util
from pathlib import Path
import stat
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

directory = Path(__file__).resolve().parents[1] / "deploy/systemd"
sys.path.insert(0, str(directory))
spec = importlib.util.spec_from_file_location("turn_stage", directory / "stage-private-turn.py")
stage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage)


class StageTests(unittest.TestCase):
    def test_credential_is_not_in_units_or_receipt_and_no_public_activation(self):
        files = stage.contents(105, "192.168.20.68", "45.24.46.155", "a"*64, "b"*64)
        self.assertEqual(len(files), 5)
        for name, data in files.items():
            self.assertEqual(b"a"*64 in data, name == "turnserver.conf")
        self.assertIn(b'"activated": false', files["stage-receipt.json"])

    def test_permissions_and_file_types_fail_closed(self):
        good = SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o600, st_nlink=1)
        stage.check_metadata(good, private=True)
        for change in [dict(st_uid=1000), dict(st_mode=stat.S_IFREG | 0o644),
                       dict(st_mode=stat.S_IFLNK | 0o600), dict(st_nlink=2)]:
            metadata = SimpleNamespace(**(vars(good) | change))
            with self.assertRaises(ValueError): stage.check_metadata(metadata, private=True)
        stage.check_metadata(SimpleNamespace(st_uid=0, st_mode=stat.S_IFDIR | 0o755), directory=True)

    def test_write_is_exclusive_and_never_truncates(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "fixture"
            stage.write_exclusive(path, b"original")
            with self.assertRaises(FileExistsError): stage.write_exclusive(path, b"replacement")
            self.assertEqual(path.read_bytes(), b"original")

    def test_matching_retry_and_conflict_preserve_files(self):
        with tempfile.TemporaryDirectory() as root, patch.object(stage, "check_metadata"):
            destination = Path(root) / "private"
            files = stage.contents(105, "192.168.20.68", "45.24.46.155", "a"*64, "b"*64)
            self.assertTrue(stage.stage(destination, files))
            self.assertFalse(stage.stage(destination, files))
            self.assertEqual(stage.existing_secret(destination), "a"*64)
            with self.assertRaises(ValueError): stage.stage(destination, files | {"egress.nft":b"changed"})
            self.assertEqual((destination / "egress.nft").read_bytes(), files["egress.nft"])
            (destination / "unexpected").touch()
            with self.assertRaises(ValueError): stage.stage(destination, files)

    def test_missing_or_duplicate_secret_never_generates_replacement(self):
        with tempfile.TemporaryDirectory() as root, patch.object(stage, "check_metadata"):
            destination = Path(root)
            for data in [b"no credential", b"static-auth-secret=" + b"a"*64 + b"\nstatic-auth-secret=" + b"b"*64]:
                (destination / "turnserver.conf").write_bytes(data)
                with self.assertRaises(ValueError): stage.existing_secret(destination)


if __name__ == "__main__": unittest.main()
