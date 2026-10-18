"""Established-media admission checks: private SQLite, read-only/fail-closed."""
from contextlib import closing
import os
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from webrtc_wan_admission import WanAdmission


@unittest.skipUnless(os.name == "posix", "production Unix private-file checks")
class AdmissionTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.file = Path(self.directory.name) / "admission.db"
        self.file.touch(mode=0o600)
        self.hash = "a"*64
        with closing(sqlite3.connect(self.file)) as db:
            db.executescript("""CREATE TABLE tester_invitations(id TEXT,created_ms INTEGER,expires_ms INTEGER,revoked INTEGER);
                CREATE TABLE tester_admissions(credential_hash TEXT,invitation_id TEXT,expires_ms INTEGER);""")
            db.execute("INSERT INTO tester_invitations VALUES ('invite',0,10000,0)")
            db.execute("INSERT INTO tester_admissions VALUES (?,'invite',9000)", (self.hash,))
            db.commit()

    def guard(self): return WanAdmission(self.file, self.hash)

    def test_valid_cache_and_revocation(self):
        guard = self.guard()
        with patch("webrtc_wan_admission.time.monotonic", return_value=10), patch("webrtc_wan_admission.time.time_ns", return_value=1000_000_000):
            self.assertTrue(guard.valid())
        with closing(sqlite3.connect(self.file)) as db:
            db.execute("UPDATE tester_invitations SET revoked=1")
            db.commit()
        with patch("webrtc_wan_admission.time.monotonic", return_value=10.5): self.assertTrue(guard.valid())
        with patch("webrtc_wan_admission.time.monotonic", return_value=11), patch("webrtc_wan_admission.time.time_ns", return_value=1000_000_000):
            self.assertFalse(guard.valid())

    def test_expiry_is_exact_and_unknown_credentials_fail(self):
        with patch("webrtc_wan_admission.time.time_ns", return_value=9000_000_000): self.assertFalse(self.guard().valid())
        with patch("webrtc_wan_admission.time.time_ns", return_value=1000_000_000):
            self.assertFalse(WanAdmission(self.file, "b"*64).valid())

    def test_missing_database_is_never_created_and_schema_errors_deny(self):
        missing = self.file.parent / "missing.db"
        self.assertFalse(WanAdmission(missing, self.hash).valid())
        self.assertFalse(missing.exists())
        with closing(sqlite3.connect(self.file)) as db:
            db.execute("DROP TABLE tester_admissions")
            db.commit()
        self.assertFalse(self.guard().valid())

    def test_public_permissions_and_symlinks_fail_closed(self):
        self.file.chmod(0o644)
        self.assertFalse(self.guard().valid())
        self.file.chmod(0o600)
        alias = self.file.parent / "alias.db"
        alias.symlink_to(self.file)
        self.assertFalse(WanAdmission(alias, self.hash).valid())

    def test_request_requires_configured_database_and_valid_fingerprint(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(ValueError): WanAdmission.from_headers({})
        with self.assertRaises(ValueError): WanAdmission(self.file, "bad")


if __name__ == "__main__": unittest.main()
