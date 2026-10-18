#!/usr/bin/env python3
"""Install the verified test artifact root-owned, without starting any service.

Default is dry-run; --apply requires sudo. Does not alter /usr/bin/turnserver,
nginx, firewall, router rules, service environment, or systemd units.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tempfile

VERSION = "4.18.0"
COMMIT = "23c6c1d32a3d2b21a56cee65d3203bcc4ea82d2b"
# /opt/4play can intentionally be user-owned for library administration.
# Privileged service executables must not live beneath that mutable prefix.
DESTINATION = Path("/opt/4play-coturn") / VERSION


def verified_artifact(directory, expected):
    if not re.fullmatch(r"[a-f0-9]{64}", expected):
        raise ValueError("Expected a SHA-256 fingerprint")
    directory = Path(directory)
    source = directory / "source/bin/turnserver"
    receipt = directory / "build-receipt.json"
    for path in [source, receipt]:
        if not stat.S_ISREG(path.lstat().st_mode):
            raise ValueError("Artifact and provenance must be regular files, not symlinks")
    if source.stat().st_size > 64*1024*1024 or receipt.stat().st_size > 4096:
        raise ValueError("Unexpected artifact size")
    metadata = json.loads(receipt.read_text(encoding="utf-8"))
    if metadata.get("version") != VERSION or metadata.get("commit") != COMMIT or Path(metadata.get("binary", "")).resolve() != source.resolve():
        raise ValueError("Build receipt does not match the pinned artifact")
    data = source.read_bytes()
    if not data.startswith(b"\x7fELF") or hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("Artifact fingerprint/type mismatch")
    return data


def private_destination(directory):
    # Check every ancestor; do not follow a symlink or install executable code
    # under a user/group/world-writable parent. Only root-owned prefixes qualify.
    for path in [*reversed(directory.parents), directory]:
        if not path.exists():
            path.mkdir(mode=0o755)
        metadata = path.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
            raise ValueError(f"Unsafe destination directory {path}: requires a root-owned directory without group/world write permission")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-directory", required=True, type=Path)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    data = verified_artifact(args.build_directory, args.sha256)
    target = DESTINATION / "turnserver"
    if not args.apply:
        print(f"Verified {VERSION} {args.sha256}; would install only {target}")
        return
    if not hasattr(os, "geteuid") or os.geteuid() != 0:
        parser.error("--apply requires sudo on Linux")
    private_destination(DESTINATION)
    if target.exists() or target.is_symlink():
        if not stat.S_ISREG(target.lstat().st_mode) or target.lstat().st_uid != 0 or target.lstat().st_mode & 0o022 or target.read_bytes() != data:
            raise ValueError("Existing binary differs or is not protected; refusing to overwrite it")
        print("Matching root-owned binary is already installed; no changes made")
        return
    descriptor, name = tempfile.mkstemp(prefix=".verified-turnserver-", dir=DESTINATION)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.chmod(0o755)
        # Exclusive publication: never replace an unexpected concurrent install.
        os.link(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)
    print(f"Installed root-owned patched binary: {target}")
    print("No service started; coturn must remain masked and the public site staged")


if __name__ == "__main__": main()
