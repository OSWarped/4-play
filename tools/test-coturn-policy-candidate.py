"""Local candidate provenance checks; never build or execute a native binary."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

script = Path(__file__).resolve().parents[1] / "deploy/systemd/build-coturn-policy-candidate.py"
spec = importlib.util.spec_from_file_location("policy_candidate", script)
candidate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(candidate)


class CandidateTests(unittest.TestCase):
    def test_patch_is_exact_and_newline_portable(self):
        data = candidate.patch_bytes()
        self.assertIn(b"s->st == UDP_SOCKET && s->sat == RELAY_SOCKET", data)
        self.assertIn(b"EPERM", data)
        self.assertIn(b"EACCES", data)
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "fixture.patch"
            path.write_bytes(data.replace(b"\n", b"\r\n"))
            self.assertEqual(candidate.patch_bytes(path), data)
            path.write_bytes(data + b"changed")
            with self.assertRaises(ValueError): candidate.patch_bytes(path)

    def test_candidate_is_distinct_from_installed_upstream_artifact(self):
        self.assertNotEqual(candidate.REVISION, candidate.builder.VERSION)
        self.assertEqual(candidate.builder.COMMIT, "23c6c1d32a3d2b21a56cee65d3203bcc4ea82d2b")


if __name__ == "__main__": unittest.main()
