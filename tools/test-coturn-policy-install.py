"""Candidate fingerprint/provenance tests; never install or execute a binary."""
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

directory = Path(__file__).resolve().parents[1] / "deploy/systemd"
sys.path.insert(0, str(directory))
spec = importlib.util.spec_from_file_location("policy_install", directory / "install-coturn-policy-candidate.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class PolicyInstallTests(unittest.TestCase):
    def test_exact_receipt_and_bytes_are_required(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "source/bin/turnserver"
            binary.parent.mkdir(parents=True)
            data = b"\x7fELFfake-test-artifact"
            binary.write_bytes(data)
            digest = hashlib.sha256(data).hexdigest()
            record = installer.receipt(digest) | {"binary":str(binary)}
            receipt = root / "candidate-receipt.json"
            receipt.write_text(json.dumps(record))
            self.assertEqual(installer.verified_candidate(root, digest), data)
            for field, bad in [("candidate_only", False), ("candidate_only", 1), ("patch_sha256", "a"*64),
                               ("commit", "different"), ("version", "4.18.0"), ("binary", str(root / "elsewhere"))]:
                receipt.write_text(json.dumps(record | {field:bad}))
                with self.assertRaises(ValueError): installer.verified_candidate(root, digest)
            receipt.write_text(json.dumps(record))
            binary.write_bytes(b"different bytes")
            with self.assertRaises(ValueError): installer.verified_candidate(root, digest)

    def test_installed_candidate_provenance_and_binary_permissions_both_verified(self):
        digest = "b"*64
        with patch.object(installer.stage, "protected_ancestors") as parents, \
             patch.object(installer.stage, "read_private", return_value=json.dumps(installer.receipt(digest)).encode()) as read, \
             patch.object(installer.stage, "verify_binary") as binary:
            self.assertEqual(installer.verify_installed(), installer.BINARY)
            parents.assert_called_once_with(installer.DESTINATION)
            binary.assert_called_once_with(digest, binary=installer.BINARY)
            read.return_value = json.dumps(installer.receipt(digest) | {"patch_sha256":"wrong"}).encode()
            with self.assertRaises(ValueError): installer.verify_installed()

    def test_candidate_cannot_replace_upstream_binary(self):
        self.assertEqual(installer.DESTINATION, Path("/opt/4play-coturn/4.18.0-4play-policy1"))
        self.assertNotEqual(installer.BINARY, installer.stage.BINARY)


if __name__ == "__main__": unittest.main()
