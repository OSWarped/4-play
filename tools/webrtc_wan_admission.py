"""Read-only, fail-closed admission checks for established WAN media peers.

Called from signaling/maintenance threads, never audio/video forwarding. Only a
domain-separated fingerprint travels from the trusted loopback control plane.
"""
import os
from contextlib import closing
from pathlib import Path
import re
import sqlite3
import stat
import time
import threading


class WanAdmission:
    def __init__(self, database, fingerprint):
        self.database = Path(database)
        self.fingerprint = fingerprint
        self.next_check = 0
        self.allowed = False
        self.lock = threading.Lock()
        if not re.fullmatch(r"[0-9a-f]{64}", fingerprint):
            raise ValueError("Missing or invalid WAN admission fingerprint")

    @classmethod
    def from_headers(cls, headers):
        database = os.environ.get("FOURPLAY_WAN_TESTER_DATABASE")
        if not database:
            raise ValueError("WAN admission database is not configured")
        guard = cls(database, headers.get("x-4play-admission-hash", ""))
        if not guard.valid():
            raise ValueError("WAN admission is not authorized")
        return guard

    def valid(self):
        with self.lock:
            return self._valid()

    def _valid(self):
        now = time.monotonic()
        if now < self.next_check:
            return self.allowed
        self.next_check = now + 1
        self.allowed = False
        try:
            metadata = self.database.lstat()
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o077:
                return False
            # Short SQLite timeout: an unavailable/locked database fails closed.
            with closing(sqlite3.connect(self.database.absolute().as_uri() + "?mode=ro", uri=True, timeout=.05)) as db:
                now_ms = time.time_ns() // 1_000_000
                row = db.execute("""SELECT 1 FROM tester_admissions a JOIN tester_invitations i
                    ON i.id=a.invitation_id WHERE a.credential_hash=? AND a.expires_ms>?
                    AND i.expires_ms>? AND i.created_ms<=? AND i.revoked=0""",
                    (self.fingerprint, now_ms, now_ms, now_ms)).fetchone()
                self.allowed = row is not None
        except (OSError, sqlite3.Error, ValueError):
            pass
        return self.allowed
