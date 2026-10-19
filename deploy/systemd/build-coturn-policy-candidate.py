#!/usr/bin/env python3
"""Build a separately labeled LOCAL PATCH CANDIDATE; never install/promote it.

The installed root-owned relay and its staging receipt remain unchanged. This
candidate is upstream 4.18.0 plus one pinned Linux relay-policy-error patch.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile

REVISION = "4.18.0-4play-policy1"
PATCH_SHA256 = "3e9fb96a00625369f612a79f9dcc79010df86583279c9fca0459c6f901c7fc5f"
PATCH = Path(__file__).with_name("coturn-relay-policy-errors.patch")
spec = importlib.util.spec_from_file_location("coturn_upstream_builder", Path(__file__).with_name("build-private-coturn.py"))
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


def patch_bytes(path=PATCH):
    data = path.read_bytes().replace(b"\r\n", b"\n")
    if hashlib.sha256(data).hexdigest() != PATCH_SHA256:
        raise ValueError("Local policy-error patch fingerprint changed; refusing to build")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", action="store_true")
    args = parser.parse_args()
    if os.name != "posix" or os.getuid() == 0:
        parser.error("Run as the ordinary Linux user, not root")
    missing = builder.missing_prerequisites()
    if missing:
        raise SystemExit("Missing prerequisites: " + ", ".join(missing))
    data = patch_bytes()
    if not args.build:
        print(f"Candidate {REVISION}: pinned upstream and local patch prerequisites present; no changes made")
        return
    root = Path(tempfile.mkdtemp(prefix=f"4play-coturn-{REVISION}-"))
    root.chmod(0o700)
    source = root / "source"
    env = builder.build_environment()
    print(f"Private candidate build directory (retained): {root}", flush=True)
    subprocess.run(["git", "clone", "--depth", "1", "--branch", builder.VERSION, builder.REPOSITORY, str(source)], check=True, env=env)
    builder.verify_checkout(source)
    subprocess.run(["git", "apply", "--check", "-"], cwd=source, input=data, check=True, env=env)
    subprocess.run(["git", "apply", "-"], cwd=source, input=data, check=True, env=env)
    subprocess.run(["./configure", f"--prefix={root}/not-installed"], cwd=source, env=env, check=True)
    subprocess.run(["make", "-j2"], cwd=source, env=env, check=True)
    binary = source / "bin/turnserver"
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    receipt = {"version":REVISION, "upstream_version":builder.VERSION, "commit":builder.COMMIT,
               "patch_sha256":PATCH_SHA256, "binary":str(binary), "binary_sha256":digest, "candidate_only":True}
    (root / "candidate-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"Built LOCAL PATCH CANDIDATE only: {binary}")
    print(f"Candidate SHA-256: {digest}")
    print("No installed binary, staging credential, service, firewall or public access changed.")


if __name__ == "__main__": main()
