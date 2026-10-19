#!/usr/bin/env python3
"""Prepare a separate deployment bundle for the tested LOCAL PATCH CANDIDATE.

Never install units, start services, change live rules/env/nginx, or overwrite
the upstream staging. --apply requires explicit local-patch acknowledgement and
reruns the confined media gate. Credentials stay root-only and are never printed.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


stage = load("deployment_baseline_stage", "stage-private-turn.py")
candidate = load("deployment_installed_candidate", "install-coturn-policy-candidate.py")
DESTINATION = Path("/etc/4play-turn-policy1")
ROOT = Path(__file__).resolve().parents[2]


def verified_baseline(uid):
    directory = stage.DESTINATION
    stage.protected_ancestors(directory)
    secret = stage.existing_secret(directory)
    encoded = stage.read_private(directory / "stage-receipt.json")
    receipt = json.loads(encoded)
    stage.verify_binary(receipt["binary_sha256"])
    expected = stage.contents(uid, receipt["local_ip"], receipt["public_ip"], secret, receipt["binary_sha256"])
    if {path.name for path in directory.iterdir()} != set(expected):
        raise ValueError("Unexpected baseline staging files")
    for name, data in expected.items():
        if stage.read_private(directory / name) != data:
            raise ValueError("Baseline staging differs from reviewed templates")
    return receipt, hashlib.sha256(encoded).hexdigest()


def contents(uid, local, public, secret, provenance, baseline_hash):
    # Keep policy/hardening identical. Only the verified executable and the
    # protected credential/rules source paths change in these inactive units.
    original = stage.contents(uid, local, public, secret, provenance["binary_sha256"])
    files = {name: data for name, data in original.items() if name != "stage-receipt.json"}
    for name in ("4play-turn.service", "4play-turn-egress.service"):
        if files[name].count(b"/etc/4play-turn/") != 1:
            raise ValueError("Reviewed service credential/rules path changed")
        if name == "4play-turn.service" and files[name].count(stage.BINARY.as_posix().encode()) != 1:
            raise ValueError("Reviewed service executable path changed")
        files[name] = files[name].replace(stage.BINARY.as_posix().encode(), candidate.BINARY.as_posix().encode()).replace(
            b"/etc/4play-turn/", b"/etc/4play-turn-policy1/")
    receipt = {
        "version": candidate.candidate.REVISION,
        "candidate_only": True, "activated": False,
        "baseline_receipt_sha256": baseline_hash,
        "binary": candidate.BINARY.as_posix(), "provenance": provenance,
        "service_uid": uid, "local_ip": local, "public_ip": public,
        "files": {name: hashlib.sha256(data).hexdigest() for name, data in files.items()},
    }
    files["deployment-receipt.json"] = (json.dumps(receipt, indent=2) + "\n").encode()
    return files


def arguments(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--describe", action="store_true", help="Print the fixed plan only; no validation or writes")
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--accept-local-patch", action="store_true", help="Acknowledge this is a downstream patch, not an upstream release")
    args = parser.parse_args(argv)
    if args.describe and (args.apply or args.accept_local_patch):
        parser.error("--describe cannot be combined with write/acknowledgement flags")
    if args.apply and not args.accept_local_patch:
        parser.error("--apply requires --accept-local-patch; candidate is not an upstream release")
    return args


def main():
    args = arguments()
    if args.describe:
        print(f"Plan only: verify baseline and installed local patch; rerun isolated media gate; stage five private files at {DESTINATION}.")
        print("Baseline, units, services, firewall, app environment, nginx and router remain unchanged. This is not public activation.")
        return
    if sys.platform != "linux" or os.geteuid() != 0:
        raise SystemExit("Verified staging requires sudo on Linux; --describe needs no privileges")
    import pwd
    account = pwd.getpwnam("turnserver")
    if account.pw_uid == 0 or os.getgrouplist(account.pw_name, account.pw_gid) != [account.pw_gid]:
        raise ValueError("Expected a dedicated non-root account without supplemental groups")
    baseline, baseline_hash = verified_baseline(account.pw_uid)
    candidate.verify_installed()
    provenance = json.loads(stage.read_private(candidate.DESTINATION / "candidate-receipt.json"))
    if not args.apply:
        print(f"Verified baseline and installed candidate. Would rerun confined media gate and stage {DESTINATION}; no changes made.")
        return
    for unit in ("coturn.service", "4play-turn.service", "4play-turn-egress.service"):
        result = subprocess.run(["/usr/bin/systemctl", "is-active", unit], capture_output=True, text=True, check=False, timeout=10)
        if result.returncode == 0 or result.stdout.strip() not in ("inactive", "unknown", "failed"):
            raise ValueError("Refusing staging while a TURN unit may be active")
    masked = subprocess.run(["/usr/bin/systemctl", "is-enabled", "coturn.service"], capture_output=True, text=True, check=False, timeout=10)
    if masked.stdout.strip() != "masked":
        raise ValueError("Debian coturn.service must remain masked")
    stage.protected_ancestors(DESTINATION.parent)
    subprocess.run([sys.executable, str(ROOT / "tools/test-private-turn-service.py"), "--policy-candidate", "--media"], check=True, timeout=150)
    # Revalidate protected inputs after the acceptance run, before generating
    # a durable credential or writing the deployment bundle.
    current, current_hash = verified_baseline(account.pw_uid)
    candidate.verify_installed()
    if current != baseline or current_hash != baseline_hash or json.loads(stage.read_private(candidate.DESTINATION / "candidate-receipt.json")) != provenance:
        raise ValueError("Protected inputs changed during validation")
    secret = stage.existing_secret(DESTINATION) if DESTINATION.exists() or DESTINATION.is_symlink() else secrets.token_hex(32)
    changed = stage.stage(DESTINATION, contents(account.pw_uid, baseline["local_ip"], baseline["public_ip"], secret, provenance, baseline_hash))
    print("Tested candidate deployment bundle created" if changed else "Matching tested candidate bundle already exists; unchanged")
    print("Credentials remain root-only. Baseline preserved; no unit installation, activation, host rules or public access changed.")


if __name__ == "__main__": main()
