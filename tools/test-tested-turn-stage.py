"""Portable candidate deployment bundle checks; no privileged or live writes."""
import importlib.util
import contextlib
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
spec = importlib.util.spec_from_file_location("tested_candidate_stage", directory / "stage-tested-turn-candidate.py")
deploy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deploy)


class DeploymentTests(unittest.TestCase):
    def files(self):
        return deploy.contents(105, "192.168.20.68", "45.24.46.155", "a"*64,
                               deploy.candidate.receipt("b"*64), "c"*64)

    def test_only_executable_and_protected_source_paths_change(self):
        files = self.files()
        original = deploy.stage.contents(105, "192.168.20.68", "45.24.46.155", "a"*64, "b"*64)
        self.assertEqual(files["turnserver.conf"], original["turnserver.conf"])
        self.assertEqual(files["egress.nft"], original["egress.nft"])
        for name in ("4play-turn.service", "4play-turn-egress.service"):
            restored = files[name].replace(b"/etc/4play-turn-policy1/", b"/etc/4play-turn/").replace(
                deploy.candidate.BINARY.as_posix().encode(), deploy.stage.BINARY.as_posix().encode())
            self.assertEqual(restored, original[name])
        self.assertIn(deploy.candidate.BINARY.as_posix().encode(), files["4play-turn.service"])
        self.assertIn(b"LoadCredential=turnserver.conf:/etc/4play-turn-policy1/turnserver.conf", files["4play-turn.service"])

    def test_manifest_has_pinned_provenance_hashes_and_no_secret(self):
        import hashlib
        files = self.files()
        receipt = json.loads(files["deployment-receipt.json"])
        self.assertEqual(len(files), 5)
        self.assertFalse(receipt["activated"])
        self.assertTrue(receipt["candidate_only"])
        self.assertEqual(receipt["provenance"], deploy.candidate.receipt("b"*64))
        self.assertEqual(receipt["baseline_receipt_sha256"], "c"*64)
        for name, data in files.items():
            self.assertEqual(b"a"*64 in data, name == "turnserver.conf")
            if name != "deployment-receipt.json":
                self.assertEqual(receipt["files"][name], hashlib.sha256(data).hexdigest())

    def test_explicit_patch_acknowledgement_is_required_for_apply(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit): deploy.arguments(["--apply"])
        self.assertTrue(deploy.arguments(["--apply", "--accept-local-patch"]).apply)
        for flags in (["--describe", "--apply"], ["--describe", "--accept-local-patch"]):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit): deploy.arguments(flags)

    def test_changed_service_template_fails_instead_of_silent_path_replacement(self):
        original = deploy.stage.contents
        def changed(*args):
            files = original(*args)
            files["4play-turn.service"] = files["4play-turn.service"].replace(deploy.stage.BINARY.as_posix().encode(), b"/unexpected/binary")
            return files
        with patch.object(deploy.stage, "contents", side_effect=changed):
            with self.assertRaisesRegex(ValueError, "executable path changed"): self.files()

    @unittest.skipUnless(sys.platform == "linux", "Linux root-path control flow with all privileged effects mocked")
    def test_media_gate_failure_prevents_durable_credential_or_bundle_creation(self):
        import pwd
        account = SimpleNamespace(pw_uid=105, pw_gid=108, pw_name="turnserver")
        args = SimpleNamespace(describe=False, apply=True, accept_local_patch=True)
        inactive = SimpleNamespace(returncode=3, stdout="inactive\n")
        masked = SimpleNamespace(returncode=1, stdout="masked\n")
        failure = deploy.subprocess.CalledProcessError(1, ["fixture"])
        with patch.object(deploy, "arguments", return_value=args), patch.object(deploy.os, "geteuid", return_value=0), patch.object(
            pwd, "getpwnam", return_value=account
        ), patch.object(deploy.os, "getgrouplist", return_value=[108]), patch.object(
            deploy, "verified_baseline", return_value=({}, "c"*64)
        ), patch.object(deploy.candidate, "verify_installed"), patch.object(
            deploy.stage, "read_private", return_value=json.dumps(deploy.candidate.receipt("b"*64)).encode()
        ), patch.object(deploy.stage, "protected_ancestors"), patch.object(
            deploy.subprocess, "run", side_effect=[inactive, inactive, inactive, masked, failure]
        ), patch.object(deploy.stage, "stage") as write, patch.object(deploy.stage, "existing_secret") as secret:
            with self.assertRaises(deploy.subprocess.CalledProcessError): deploy.main()
            write.assert_not_called(); secret.assert_not_called()

    def test_idempotent_bundle_does_not_overwrite_conflict_or_baseline(self):
        with tempfile.TemporaryDirectory() as root, patch.object(deploy.stage, "check_metadata"):
            root = Path(root)
            baseline = root / "baseline"
            baseline.mkdir()
            deploy.stage.write_exclusive(baseline / "retain", b"baseline")
            destination = root / "candidate"
            files = self.files()
            self.assertTrue(deploy.stage.stage(destination, files))
            self.assertFalse(deploy.stage.stage(destination, files))
            with self.assertRaises(ValueError):
                deploy.stage.stage(destination, files | {"egress.nft": b"conflict"})
            self.assertEqual((destination / "egress.nft").read_bytes(), files["egress.nft"])
            self.assertEqual((baseline / "retain").read_bytes(), b"baseline")

    def test_description_never_reads_root_files_or_runs_services(self):
        with patch.object(sys, "argv", ["stage", "--describe"]), patch.object(deploy, "verified_baseline") as verify, patch.object(deploy.subprocess, "run") as run:
            deploy.main()
            verify.assert_not_called(); run.assert_not_called()


if __name__ == "__main__": unittest.main()
