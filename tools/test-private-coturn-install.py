"""Artifact verification tests; never install or execute third-party binaries."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "deploy/systemd/install-private-coturn.py"
spec = importlib.util.spec_from_file_location("private_coturn_install", SCRIPT)
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class InstallTests(unittest.TestCase):
    def test_verified_bytes_are_a_snapshot_of_the_pinned_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source/bin/turnserver"
            source.parent.mkdir(parents=True)
            data = b"\x7fELFtest-artifact-not-executable"
            source.write_bytes(data)
            receipt = root / "build-receipt.json"
            metadata = {"version":installer.VERSION,"commit":installer.COMMIT,"binary":str(source)}
            receipt.write_text(json.dumps(metadata), encoding="utf-8")
            digest = hashlib.sha256(data).hexdigest()
            self.assertEqual(installer.verified_artifact(root, digest), data)
            for incorrect in ["wrong", "a"*64]:
                with self.assertRaises(ValueError): installer.verified_artifact(root, incorrect)
            metadata["commit"] = "changed"
            receipt.write_text(json.dumps(metadata), encoding="utf-8")
            with self.assertRaises(ValueError): installer.verified_artifact(root, digest)
            metadata["commit"] = installer.COMMIT
            receipt.write_text(json.dumps(metadata), encoding="utf-8")
            source.write_bytes(b"different bytes")
            with self.assertRaises(ValueError): installer.verified_artifact(root, digest)


if __name__ == "__main__": unittest.main()
