#!/usr/bin/env python3
"""Check prerequisites or build a pinned coturn, privately and without sudo/install.

The Debian coturn service must remain masked. This tool never starts it or edits
system configuration. Failed/successful build directories are retained for review.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

VERSION = "4.18.0"
COMMIT = "23c6c1d32a3d2b21a56cee65d3203bcc4ea82d2b"
REPOSITORY = "https://github.com/coturn/coturn.git"


def missing_prerequisites(which=shutil.which, run=subprocess.run):
    missing = [name for name in ["git", "cc", "make", "pkg-config"] if not which(name)]
    if not which("pkg-config"):
        return missing
    for library, package in [("libevent", "libevent-dev"), ("openssl", "libssl-dev"), ("sqlite3", "libsqlite3-dev")]:
        result = run(["pkg-config", "--exists", library], check=False)
        if result.returncode:
            missing.append(package)
    return missing


def verify_checkout(source, run=subprocess.run):
    revision = run(["git", "-C", str(source), "rev-parse", "HEAD"], check=True, capture_output=True, text=True).stdout.strip()
    if revision != COMMIT:
        raise RuntimeError("Coturn tag does not match the reviewed pinned source commit; refusing to execute it")


def build_environment():
    # Do not pass the 4-Play service environment, credentials, or user-provided
    # compiler injection variables into third-party configure/make steps.
    env = {name: os.environ[name] for name in ["PATH", "HOME", "TMPDIR"] if name in os.environ}
    env.update({name: "1" for name in ["TURN_NO_PROMETHEUS", "TURN_NO_SYSTEMD", "TURN_NO_MYSQL", "TURN_NO_PQ", "TURN_NO_MONGO", "TURN_NO_HIREDIS", "TURN_NO_SCTP"]})
    return env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", action="store_true", help="Build in a new private temporary directory; do not install/start")
    args = parser.parse_args()
    if os.name != "posix" or os.getuid() == 0:
        parser.error("Run as the ordinary Linux service user, not root")
    missing = missing_prerequisites()
    if missing:
        raise SystemExit("Missing build prerequisites: " + ", ".join(missing))
    print(f"Coturn {VERSION} build prerequisites present; source pinned to {COMMIT}")
    if not args.build:
        return
    root = Path(tempfile.mkdtemp(prefix=f"4play-coturn-{VERSION}-"))
    root.chmod(0o700)
    source = root / "source"
    print(f"Private build directory (retained): {root}", flush=True)
    env = build_environment()
    subprocess.run(["git", "clone", "--depth", "1", "--branch", VERSION, REPOSITORY, str(source)], check=True, env=env)
    verify_checkout(source)
    subprocess.run(["./configure", f"--prefix={root}/not-installed"], cwd=source, env=env, check=True)
    subprocess.run(["make", "-j2"], cwd=source, env=env, check=True)
    binary = source / "bin/turnserver"
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise RuntimeError("Pinned build did not produce an executable turnserver")
    # Receipt contains provenance only; no test/production credentials.
    (root / "build-receipt.json").write_text(json.dumps({"version":VERSION, "commit":COMMIT, "binary":str(binary)}, indent=2) + "\n", encoding="utf-8")
    print(f"Built (not installed or started): {binary}")
    print("This is a temporary test artifact, not a persistent service deployment.")


if __name__ == "__main__":
    main()
