#!/usr/bin/env python3
"""Switch only the managed arcade site to invitation-protected HTTPS, with rollback.

No TURN guard, UFW/router, application service or unrelated nginx site changes.
The existing managed site filename/symlink is retained; only its exact known
contents are eligible for replacement. Public gameplay is not yet media-ready.
"""
import argparse
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy/systemd"))


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


staging = load("protected_edge_staging", Path(__file__).with_name("prepare-public-edge.py"))
edge = load("protected_edge_checks", ROOT / "tools/check-local-wan-edge.py")
setup = load("protected_edge_enrollment", ROOT / "deploy/systemd/configure-wan-enrollment.py")


def run(args):
    return subprocess.run(args, check=True, capture_output=True, timeout=180)


def candidate():
    return Path(__file__).with_name("arcade-public.conf.example").read_bytes()


def preflight():
    for parent in (staging.SITE.parent, staging.ENABLED.parent):
        setup.delivery.stage.protected_ancestors(parent)
    metadata = staging.SITE.lstat()
    setup.delivery.stage.check_metadata(metadata)
    if metadata.st_size > 65536:
        raise ValueError("Unexpected managed site size")
    previous = staging.SITE.read_bytes()
    if previous not in (staging.render(True).encode(), candidate()):
        raise ValueError("Unknown/customized arcade configuration")
    if not staging.ENABLED.is_symlink() or staging.ENABLED.readlink() != staging.SITE or staging.ENABLED.lstat().st_uid != 0:
        raise ValueError("Unknown enabled arcade site")
    return previous


def verify_public():
    wait_for_status(200)
    for path in ("/client-v4", "/client-v2/tester-access", "/client/app.js"):
        edge.require(edge.request(443, path), 200)
    for path in ("/admin", "/api/v1/games", "/api/v1/sessions", "/sessions/123/offer"):
        edge.require(edge.request(443, path), 404)
    edge.require(edge.request(443, "/client-v2/bootstrap", method="POST", data={}), 401, "tester_access_required")
    edge.require(edge.request(443, "/client-v2/api/games"), 401, "tester_access_required")
    print("PASS: live HTTPS serves client/login, requires invitation and blocks admin/raw APIs")


def verify_staging():
    wait_for_status(503)
    print("PASS: public arcade HTTPS restored to staging 503")


def wait_for_status(status):
    # nginx reload is asynchronous; allow old workers a short handover window.
    deadline = time.monotonic() + 5
    while True:
        response = edge.request(443, "/client-v4")
        if response[0] == status: return
        if time.monotonic() >= deadline: edge.require(response, status)
        time.sleep(.1)


def switch(target, verify):
    previous = preflight()
    if target not in (staging.render(True).encode(), candidate()):
        raise ValueError("Unknown target configuration")
    run(["/usr/sbin/nginx", "-t"])
    if target == previous:
        verify()
        return
    # atomic_write changes only the exact known managed site, never its symlink,
    # certificates, private snippet, other virtual hosts or application settings.
    try:
        staging.atomic_write(staging.SITE, target)
        run(["/usr/sbin/nginx", "-t"])
        run(["/usr/bin/systemctl", "reload", "nginx"])
        verify()
    except BaseException:
        # Refuse to overwrite a third party's concurrent modification.
        current = preflight()
        if current not in (previous, target):
            raise RuntimeError("Managed site changed during rollback") from None
        staging.atomic_write(staging.SITE, previous)
        run(["/usr/sbin/nginx", "-t"])
        run(["/usr/bin/systemctl", "reload", "nginx"])
        print("ROLLBACK: restored and reloaded the previous managed arcade configuration")
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--restore-staging", action="store_true")
    args = parser.parse_args()
    if not args.apply:
        print(__doc__)
        print("Plan only. --apply runs the local admission gate before switching; --apply --restore-staging rolls back.")
        return
    if sys.platform != "linux" or os.geteuid() != 0:
        parser.error("--apply requires sudo on the reference Linux server")
    previous = preflight()
    if args.restore_staging:
        switch(staging.render(True).encode(), verify_staging)
        return
    if previous == candidate():
        verify_public()
        print("Protected site already installed; no configuration changed")
        return
    # Reprove authentication with the real keys and app processes immediately
    # before public activation. No secret-bearing output is forwarded.
    run(["/usr/bin/python3", str(ROOT / "tools/check-local-wan-edge.py"), "--run"])
    print("PASS: real local HTTPS invitation/revocation gate reconfirmed")
    switch(candidate(), verify_public)
    print("Protected public client enabled. Media ports/guard unchanged; external gameplay is NOT ready yet.")


if __name__ == "__main__":
    try:
        main()
    except (Exception, KeyboardInterrupt):
        raise SystemExit("Protected edge operation failed; secret-bearing details suppressed. Check token-safe rollback output; do not open media ports.") from None
