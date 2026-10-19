#!/usr/bin/env python3
"""Install verified TURN unit definitions, inactive and disabled.

No relay start/enable, nft/UFW change, app environment or nginx mutation. Fixed
paths only; unrelated/existing conflicting units are never overwritten.
"""
import argparse
import json
import os
from pathlib import Path
import stat
import subprocess
import sys

import importlib.util
spec = importlib.util.spec_from_file_location("tested_turn_bundle", Path(__file__).with_name("stage-tested-turn-candidate.py"))
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)
stage = bundle.stage
TARGET = Path("/etc/systemd/system")
NAMES = ("4play-turn-egress.service", "4play-turn.service")


def verify_bundle(uid):
    baseline, baseline_hash = bundle.verified_baseline(uid)
    bundle.candidate.verify_installed()
    provenance = json.loads(stage.read_private(bundle.candidate.DESTINATION / "candidate-receipt.json"))
    stage.protected_ancestors(bundle.DESTINATION)
    secret = stage.existing_secret(bundle.DESTINATION)
    expected = bundle.contents(uid, baseline["local_ip"], baseline["public_ip"], secret, provenance, baseline_hash)
    if {path.name for path in bundle.DESTINATION.iterdir()} != set(expected):
        raise ValueError("Unexpected candidate bundle files")
    for name, data in expected.items():
        if stage.read_private(bundle.DESTINATION / name) != data:
            raise ValueError("Candidate deployment bundle differs from reviewed inputs")
    return {name: expected[name] for name in NAMES}


def check_targets(files):
    stage.protected_ancestors(TARGET)
    for name in NAMES:
        path = TARGET / name
        if path.exists() or path.is_symlink():
            stage.check_metadata(path.lstat())
            if path.read_bytes() != files[name]:
                raise ValueError(f"Conflicting unit: {name}; refusing overwrite")


def check_inactive():
    for name in (*NAMES, "coturn.service"):
        status = subprocess.run(["/usr/bin/systemctl", "is-active", name], capture_output=True, text=True, check=False, timeout=10)
        if status.returncode == 0 or status.stdout.strip() not in ("inactive", "unknown", "failed"):
            raise ValueError("Refusing installation while a TURN unit may be active")
        enabled = subprocess.run(["/usr/bin/systemctl", "is-enabled", name], capture_output=True, text=True, check=False, timeout=10)
        allowed = ("masked",) if name == "coturn.service" else ("disabled", "static", "not-found", "")
        if enabled.stdout.strip() not in allowed or (not enabled.stdout.strip() and enabled.returncode not in (1, 4)):
            raise ValueError("TURN units must remain disabled and Debian coturn masked")


def install(files):
    """Create only absent, exact-name files; roll back our inodes on failure."""
    created = []
    try:
        check_targets(files)  # Preflight ALL collisions before the first write.
        for name in NAMES:
            path = TARGET / name
            if path.exists():
                continue
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
            with os.fdopen(descriptor, "wb") as stream:
                metadata = os.fstat(stream.fileno())
                created.append((path, metadata.st_dev, metadata.st_ino))
                stream.write(files[name]); stream.flush(); os.fsync(stream.fileno())
                os.fchmod(stream.fileno(), 0o644)
        subprocess.run(["/usr/bin/systemctl", "daemon-reload"], check=True, timeout=30)
        check_targets(files)
        check_inactive()
    except Exception:
        for path, device, inode in reversed(created):
            metadata = path.lstat()
            if (metadata.st_dev, metadata.st_ino) != (device, inode) or not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
                raise RuntimeError("A new unit changed during rollback; refusing removal") from None
            path.unlink()
        # Reload the restored definitions, but never start/stop any service.
        if created:
            subprocess.run(["/usr/bin/systemctl", "daemon-reload"], check=True, timeout=30)
        raise
    return len(created)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--describe", action="store_true")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    if args.describe and args.apply:
        parser.error("--describe cannot be combined with --apply")
    if args.describe:
        print("Plan only: verify tested candidate bundle; install two fixed unit files inactive/disabled; daemon-reload only.")
        print("No service start/enable, host firewall, application environment, nginx, router or public access changes.")
        return
    if sys.platform != "linux" or os.geteuid() != 0:
        parser.error("Verified installation requires sudo on Linux; --describe is unprivileged")
    import pwd
    account = pwd.getpwnam("turnserver")
    if account.pw_uid == 0 or os.getgrouplist(account.pw_name, account.pw_gid) != [account.pw_gid]:
        raise ValueError("Expected a dedicated non-root service account without supplemental groups")
    files = verify_bundle(account.pw_uid)
    check_targets(files)
    check_inactive()
    subprocess.run(["/usr/bin/systemd-analyze", "verify", *(str(bundle.DESTINATION / name) for name in NAMES)], check=True, timeout=30)
    if not args.apply:
        print("Verified bundle, inactive units and syntax; no files written. --apply installs units but does not activate them.")
        return
    if verify_bundle(account.pw_uid) != files:
        raise ValueError("Candidate bundle changed during validation")
    check_inactive()
    count = install(files)
    print(f"Installed {count} new verified unit definition(s); matching existing files retained.")
    print("Units remain inactive/disabled; Debian coturn remains masked. No host firewall or public access activated.")


if __name__ == "__main__": main()
