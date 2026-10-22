#!/usr/bin/env python3
"""Prepare private WAN enrollment and API-token overrides; never restart or expose services."""
import argparse
import importlib.util
import os
from pathlib import Path
import re
import secrets
import stat
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
SETTINGS = Path("/etc/4play-wan-enrollment")
UNITS = Path("/etc/systemd/system")
SNIPPET = Path("/etc/nginx/snippets/4play-wan-edge-key.conf")
DATABASE = Path("/home/blake/.local/share/4play/tester-access/access.sqlite3")
ORIGIN = "https://arcade.blakemilam.com"
DROPIN = "45-4play-wan-enrollment.conf"
spec = importlib.util.spec_from_file_location("enrollment_media_delivery", Path(__file__).with_name("install-wan-media-credentials.py"))
delivery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(delivery)


def run(args, **kwargs):
    return subprocess.run(args, check=True, timeout=45, **kwargs)


def contents(seat, host, edge):
    if len({seat, host, edge}) != 3 or not all(re.fullmatch(r"[a-f0-9]{64}", value) for value in (seat, host, edge)):
        raise ValueError("Expected three distinct random 64-hex keys")
    files = {
        SETTINGS / "service-tokens.env":
            (f"FOURPLAY_SEAT_API_TOKEN={seat}\nFOURPLAY_RUNTIME_HOST_API_TOKEN={host}\n".encode(), 0o600),
        SETTINGS / "control-plane.env":
            (f"FOURPLAY_WAN_TESTER_ORIGIN={ORIGIN}\nFOURPLAY_WAN_EDGE_KEY={edge}\nFOURPLAY_WAN_TESTER_DATABASE={DATABASE}\n".encode(), 0o600),
        SETTINGS / "runtime-host.env":
            (f"FOURPLAY_WAN_TESTER_DATABASE={DATABASE}\n".encode(), 0o600),
        SNIPPET: (f'proxy_set_header X-4Play-Edge-Key "{edge}";\n'.encode(), 0o600),
    }
    for index, service in enumerate(delivery.SERVICES):
        environment = SETTINGS / ("control-plane.env" if index == 0 else "runtime-host.env")
        files[UNITS / (service + ".d") / DROPIN] = (
            f"[Service]\nEnvironmentFile={SETTINGS}/service-tokens.env\nEnvironmentFile={environment}\n".encode(), 0o644)
    return files


def material():
    paths = [SETTINGS / name for name in ("service-tokens.env", "control-plane.env", "runtime-host.env")]
    present = [path.exists() or path.is_symlink() for path in paths]
    if not any(present):
        return tuple(secrets.token_hex(32) for _ in range(3))
    if not all(present):
        raise ValueError("Partial enrollment configuration; review before proceeding")
    settings = {}
    for path in paths:
        delivery.stage.protected_ancestors(path.parent)
        delivery.stage.check_metadata(path.parent.lstat(), directory=True, private=True)
        data = delivery.stage.read_private(path)
        if len(data) > 65536:
            raise ValueError("Enrollment settings exceed size limit")
        for line in data.decode().splitlines():
            key, separator, value = line.partition("=")
            if not separator:
                raise ValueError("Malformed enrollment settings")
            if key in settings and settings[key] != value:
                raise ValueError("Conflicting enrollment settings")
            settings[key] = value
    try:
        return tuple(settings[key] for key in ("FOURPLAY_SEAT_API_TOKEN", "FOURPLAY_RUNTIME_HOST_API_TOKEN", "FOURPLAY_WAN_EDGE_KEY"))
    except KeyError:
        raise ValueError("Incomplete enrollment settings") from None


def verify_host():
    import pwd
    uid = pwd.getpwnam("blake").pw_uid
    if uid == 0:
        raise ValueError("Expected non-root application identity")
    for service in delivery.SERVICES:
        unit = UNITS / service
        delivery.stage.protected_ancestors(unit.parent)
        delivery.stage.check_metadata(unit.lstat())
        expected_dropins = [str(unit.parent / (service + ".d") / delivery.DROPIN)]
        ours = unit.parent / (service + ".d") / DROPIN
        if ours.exists() or ours.is_symlink(): expected_dropins.append(str(ours))
        expected_env = ["/etc/4play/4play.env (ignore_errors=no)",
                        f"/etc/4play-wan-media/{service.removesuffix('.service')}.env (ignore_errors=no)"]
        if len(expected_dropins) == 2:
            environment = "control-plane.env" if service == delivery.SERVICES[0] else "runtime-host.env"
            expected_env.extend([f"{SETTINGS}/service-tokens.env (ignore_errors=no)", f"{SETTINGS}/{environment} (ignore_errors=no)"])
        expected = {"User": "blake", "FragmentPath": str(unit), "NeedDaemonReload": "no",
                    "DropInPaths": " ".join(expected_dropins), "EnvironmentFiles": "\n".join(expected_env),
                    "UnsetEnvironment": "FOURPLAY_WAN_TURN_SECRET"}
        for prop, wanted in expected.items():
            actual = run(["/usr/bin/systemctl", "show", service, "--property=" + prop, "--value"], capture_output=True, text=True).stdout.strip()
            if actual != wanted:
                raise ValueError(f"Unexpected {prop} on {service}; no configuration installed")
        # Confirm the already-installed TURN credential settings are unchanged.
        path = Path("/etc/4play-wan-media") / (service.removesuffix(".service") + ".env")
        delivery.stage.read_private(path)
    print("PASS: application identities, ordered environment files and known drop-ins")
    for service in (*delivery.SERVICES, "4play-turn.service", "4play-turn-egress.service"):
        if run(["/usr/bin/systemctl", "is-active", service], capture_output=True, text=True).stdout.strip() != "active":
            raise ValueError("Application and guarded TURN services must be active")
    run(["/usr/sbin/nft", "list", "table", "inet", "fourplay_turn_local_only"], capture_output=True)
    run(["/usr/sbin/nft", "list", "table", "inet", "fourplay_turn"], capture_output=True)
    print("PASS: active application/TURN services and dedicated TURN guard tables")
    for parent in DATABASE.parents:
        metadata = parent.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_mode & 0o022 or metadata.st_uid not in (0, uid):
            raise ValueError("Unsafe invitation database parent")
    metadata = DATABASE.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o077 or metadata.st_uid != uid or metadata.st_nlink != 1 or metadata.st_size > 16*1024*1024:
        raise ValueError("Invitation database must be private and owned by the application user")
    run(["/usr/sbin/runuser", "--user", "blake", "--", str(ROOT / "target/release/4play-tester-access"),
         "list", "--database", str(DATABASE)], capture_output=True)
    print("PASS: private invitation database readable by the application user")
    status = run(["/usr/bin/curl", "--silent", "--show-error", "--max-time", "10", "--output", "/dev/null",
                  "--write-out", "%{http_code}", "--resolve", "arcade.blakemilam.com:443:127.0.0.1",
                  ORIGIN + "/client-v4"], capture_output=True, text=True).stdout
    if status != "503":
        raise ValueError("Arcade HTTPS must still be staged (503)")
    print("PASS: arcade HTTPS still returns staging 503")


def install(files):
    # Reuse the tested exclusive-create, identity-checked rollback transaction.
    original_settings, original_units = delivery.SETTINGS, delivery.UNITS
    try:
        delivery.SETTINGS, delivery.UNITS = SETTINGS, UNITS
        return delivery.install(files)
    finally:
        delivery.SETTINGS, delivery.UNITS = original_settings, original_units


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--describe", action="store_true")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    if args.apply and args.describe: parser.error("Choose --describe or --apply")
    if args.describe:
        print("Plan: generate/reuse three private keys; configure exact HTTPS origin and shared invitation database;")
        print("install two later environment drop-ins and an UNUSED nginx snippet; daemon-reload only.")
        print("No restarts, nginx activation/reload, firewall/router changes, invitation issuance or secret output.")
        return
    if sys.platform != "linux" or os.geteuid() != 0: parser.error("Run with sudo on the reference Linux server")
    verify_host()
    files = contents(*material())
    # All conflicts must be rejected even for a dry run.
    original_settings = delivery.SETTINGS
    try:
        delivery.SETTINGS = SETTINGS
        delivery.preflight(files)
    finally:
        delivery.SETTINGS = original_settings
    if not args.apply:
        print("Preflight passed. No files written. Use --apply to stage private enrollment settings.")
        return
    count = install(files)
    print(f"Installed {count} new private enrollment configuration file(s); matching files retained.")
    print("New service tokens take effect ONLY after restarting both applications. Old token-based clients/admin tools then require updating.")
    print("No secrets printed; original environment retained for rollback. Public HTTPS remains staged; firewall/router unchanged.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError):
        # Never include exception data: it may contain parsed private settings.
        raise SystemExit("Enrollment setup failed; no secrets printed. Review preflight requirements or conflicting managed files.") from None
