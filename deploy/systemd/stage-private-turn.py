#!/usr/bin/env python3
"""Stage root-only TURN credentials/config/units after isolated validation.

Default is dry-run. --apply writes ONLY /etc/4play-turn; it never installs units
in systemd's search path, starts services, loads host rules, or edits app/nginx
environment. No secret is accepted in command arguments or printed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import stat
import subprocess
import sys

from private_turn_bundle import VERSION, addresses, egress_rules, service_units, turn_config

DESTINATION = Path("/etc/4play-turn")
BINARY = Path(f"/opt/4play-coturn/{VERSION}/turnserver")
REFERENCE_SHA256 = "afca6458e0bd736bb485eb6f934732fbd2cc6b51cdb6538b046ef8cda38b9b86"


def check_metadata(metadata, *, directory=False, private=False):
    kind = stat.S_ISDIR if directory else stat.S_ISREG
    forbidden = 0o077 if private else 0o022
    if not kind(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & forbidden:
        raise ValueError("Expected a protected root-owned directory/file, not a symlink")
    if not directory and metadata.st_nlink != 1:
        raise ValueError("Refusing a file with multiple hard links")


def protected_ancestors(directory):
    for path in [*reversed(directory.parents), directory]:
        check_metadata(path.lstat(), directory=True)


def verify_binary(expected):
    if not re.fullmatch(r"[a-f0-9]{64}", expected):
        raise ValueError("Expected a SHA-256 fingerprint")
    protected_ancestors(BINARY.parent)
    metadata = BINARY.lstat()
    check_metadata(metadata)
    if metadata.st_size > 64 * 1024 * 1024 or metadata.st_mode & 0o111 != 0o111:
        raise ValueError("Unexpected binary size or executable permissions")
    data = BINARY.read_bytes()
    if not data.startswith(b"\x7fELF") or hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("Installed binary does not match the tested fingerprint")


def contents(uid, local, public, secret, digest):
    files = {
        "turnserver.conf": turn_config(local, public, secret),
        "egress.nft": egress_rules(uid, local, public),
        **service_units(),
    }
    files["stage-receipt.json"] = json.dumps({
        "version": VERSION, "binary_sha256": digest, "service_uid": uid,
        "local_ip": local, "public_ip": public, "activated": False,
        "files": {name:hashlib.sha256(text.encode()).hexdigest() for name, text in files.items()},
    }, indent=2) + "\n"
    return {name:text.encode() for name, text in files.items()}


def read_private(path):
    metadata = path.lstat()
    check_metadata(metadata, private=True)
    if metadata.st_size > 65536:
        raise ValueError("Unexpected staged file size")
    return path.read_bytes()


def existing_secret(directory):
    check_metadata(directory.lstat(), directory=True, private=True)
    text = read_private(directory / "turnserver.conf").decode("utf-8")
    found = re.findall(r"^static-auth-secret=([a-f0-9]{64})$", text, re.MULTILINE)
    if len(found) != 1:
        raise ValueError("Existing TURN credential is missing or ambiguous; refusing to replace it")
    return found[0]


def write_exclusive(path, data):
    # All parents were verified root-only. Never truncate an existing file.
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


def stage(directory, files):
    if directory.exists() or directory.is_symlink():
        check_metadata(directory.lstat(), directory=True, private=True)
        if {path.name for path in directory.iterdir()} != set(files):
            raise ValueError("Existing staging directory has unexpected/incomplete contents; refusing changes")
        for name, data in files.items():
            if read_private(directory / name) != data:
                raise ValueError(f"Existing staged file differs: {name}; refusing to overwrite it")
        return False
    directory.mkdir(mode=0o700)
    check_metadata(directory.lstat(), directory=True, private=True)
    for name, data in files.items():
        write_exclusive(directory / name, data)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--local-ip", required=True)
    parser.add_argument("--public-ip", required=True)
    parser.add_argument("--binary-sha256", default=REFERENCE_SHA256)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("Run on the Linux runtime host")
    local, public = addresses(args.local_ip, args.public_ip)
    verify_binary(args.binary_sha256)
    import pwd
    account = pwd.getpwnam("turnserver")
    if account.pw_uid == 0 or os.getgrouplist(account.pw_name, account.pw_gid) != [account.pw_gid]:
        raise ValueError("TURN needs a non-root dedicated account without supplemental groups")
    if not args.apply:
        print(f"Verified protected binary; would validate isolated egress and stage private files at {DESTINATION}")
        print("No changes made; --apply requires sudo. No service/network activation is performed.")
        return
    if os.geteuid() != 0:
        parser.error("--apply requires sudo")
    masked = subprocess.run(["/usr/bin/systemctl", "is-enabled", "coturn.service"], capture_output=True, text=True, check=False)
    if masked.stdout.strip() != "masked":
        raise ValueError("Debian coturn.service must remain persistently masked")
    for unit in service_units():
        active = subprocess.run(["/usr/bin/systemctl", "is-active", unit], capture_output=True, text=True, check=False)
        if active.returncode == 0 or active.stdout.strip() not in ("inactive", "unknown", "failed"):
            raise ValueError("Refusing staging while a private TURN unit may be active")
    protected_ancestors(DESTINATION.parent)
    # Do not publish persistent credentials until the actual kernel policy test
    # and systemd syntax checks have succeeded outside the host namespace.
    test = Path(__file__).resolve().parents[2] / "tools/test-private-turn-egress.py"
    subprocess.run([sys.executable, str(test)], check=True, timeout=60)
    secret = existing_secret(DESTINATION) if DESTINATION.exists() or DESTINATION.is_symlink() else secrets.token_hex(32)
    changed = stage(DESTINATION, contents(account.pw_uid, local, public, secret, args.binary_sha256))
    print("Private TURN staging created" if changed else "Matching private TURN staging already exists; unchanged")
    print("Credentials remain root-only; no secret was printed.")
    print("No units installed/enabled, services started, host firewall loaded, app environment edited, or public access enabled.")


if __name__ == "__main__": main()
