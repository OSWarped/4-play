#!/usr/bin/env python3
"""Copy only the verified LOCAL PATCH CANDIDATE into its own root-owned path.

Never replace the upstream binary, update service staging, or activate anything.
Default is dry-run; --apply requires sudo. No third-party binary is run as root.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import tempfile


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


candidate = load("policy_candidate_builder", "build-coturn-policy-candidate.py")
installer = load("upstream_private_installer", "install-private-coturn.py")
stage = load("turn_stage_candidate", "stage-private-turn.py")
DESTINATION = Path("/opt/4play-coturn") / candidate.REVISION
BINARY = DESTINATION / "turnserver"


def receipt(digest):
    return {"version":candidate.REVISION, "upstream_version":candidate.builder.VERSION,
            "commit":candidate.builder.COMMIT, "patch_sha256":candidate.PATCH_SHA256,
            "binary_sha256":digest, "candidate_only":True}


def verified_candidate(directory, expected):
    if not re.fullmatch(r"[a-f0-9]{64}", expected):
        raise ValueError("Expected a SHA-256 fingerprint")
    directory = Path(directory)
    source = directory / "source/bin/turnserver"
    provenance = directory / "candidate-receipt.json"
    for path, limit in [(source, 64*1024*1024), (provenance, 4096)]:
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
            raise ValueError("Expected bounded regular candidate files, not symlinks")
    record = json.loads(provenance.read_text())
    if record.get("candidate_only") is not True or any(record.get(key) != value for key, value in receipt(expected).items()):
        raise ValueError("Candidate provenance differs from the pinned local patch")
    if Path(record.get("binary", "")).resolve() != source.resolve():
        raise ValueError("Candidate receipt points to a different artifact")
    data = source.read_bytes()
    if not data.startswith(b"\x7fELF") or hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("Candidate fingerprint/type mismatch")
    return data


def verify_installed():
    stage.protected_ancestors(DESTINATION)
    record = json.loads(stage.read_private(DESTINATION / "candidate-receipt.json"))
    digest = record.get("binary_sha256", "")
    if record.get("candidate_only") is not True or record != receipt(digest):
        raise ValueError("Installed candidate provenance mismatch")
    stage.verify_binary(digest, binary=BINARY)
    return BINARY


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-directory", required=True, type=Path)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    data = verified_candidate(args.build_directory, args.sha256)
    if not args.apply:
        print(f"Verified local patch candidate; would copy only into {DESTINATION}")
        return
    if not hasattr(os, "geteuid") or os.geteuid() != 0:
        parser.error("--apply requires sudo on Linux")
    installer.private_destination(DESTINATION)
    provenance = DESTINATION / "candidate-receipt.json"
    encoded = (json.dumps(receipt(args.sha256), indent=2) + "\n").encode()
    # Check BOTH existing files before any write; never silently overwrite an
    # unrelated binary or conflicting receipt, even after a partial install.
    if BINARY.exists() or BINARY.is_symlink():
        stage.check_metadata(BINARY.lstat())
        if BINARY.read_bytes() != data:
            raise ValueError("Existing candidate binary differs; refusing overwrite")
    if provenance.exists() or provenance.is_symlink():
        if stage.read_private(provenance) != encoded:
            raise ValueError("Existing candidate receipt differs; refusing overwrite")
    if not BINARY.exists():
        descriptor, name = tempfile.mkstemp(prefix=".candidate-", dir=DESTINATION)
        temporary = Path(name)
        try:
            with os.fdopen(descriptor, "wb") as stream:
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
            temporary.chmod(0o755)
            os.link(temporary, BINARY)
        finally:
            temporary.unlink(missing_ok=True)
    if not provenance.exists():
        stage.write_exclusive(provenance, encoded)
    verify_installed()
    print(f"Verified root-owned LOCAL PATCH CANDIDATE: {BINARY}")
    print("Upstream binary and staging unchanged; no service, host firewall or public access activated.")


if __name__ == "__main__": main()
