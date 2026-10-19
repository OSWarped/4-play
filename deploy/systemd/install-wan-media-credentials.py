#!/usr/bin/env python3
"""Install secret-free app drop-ins pointing to systemd TURN credentials.

No service restart, application token rotation, public edge or network changes.
First prove Rust/Python credential readers under actual service identities.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[2]
SETTINGS = Path("/etc/4play-wan-media")
UNITS = Path("/etc/systemd/system")
SERVICES = ("4play-control-plane.service", "4play-runtime-host-agent.service")
DROPIN = "40-4play-wan-media.conf"
spec = importlib.util.spec_from_file_location("media_turn_units", Path(__file__).with_name("install-tested-turn-units.py"))
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)
stage = installer.stage


def run(args, **kwargs):
    return subprocess.run(args, check=True, timeout=45, **kwargs)


def contents(local, public):
    files = {}
    for service in SERVICES:
        host = "arcade.blakemilam.com" if service == SERVICES[0] else local
        env = f"FOURPLAY_WAN_TURN_HOST={host}\nFOURPLAY_WAN_TURN_CREDENTIAL=4play-turn.conf\n"
        if service == SERVICES[1]: env += f"FOURPLAY_WAN_PUBLIC_IP={public}\n"
        environment = SETTINGS / (service.removesuffix(".service") + ".env")
        files[environment] = (env.encode(), 0o600)
        definition = ("[Service]\nLoadCredential=4play-turn.conf:/etc/4play-turn-policy1/turnserver.conf\n"
                      f"EnvironmentFile={environment.as_posix()}\nUnsetEnvironment=FOURPLAY_WAN_TURN_SECRET\n")
        files[UNITS / (service + ".d") / DROPIN] = (definition.encode(), 0o644)
    return files


def credential_command(service, user, local, public):
    unit = "4play-media-credential-check-" + uuid.uuid4().hex + ".service"
    command = ["/usr/bin/systemd-run", "--quiet", "--wait", "--collect", "--pipe", "--unit=" + unit,
               "--property=User=" + user, "--property=NoNewPrivileges=yes", "--property=CapabilityBoundingSet=",
               "--property=AmbientCapabilities=", "--property=PrivateTmp=yes", "--property=ProtectSystem=strict",
               "--property=ProtectHome=read-only", "--property=RestrictAddressFamilies=AF_UNIX", "--property=RuntimeMaxSec=15s",
               "--property=LoadCredential=4play-turn.conf:/etc/4play-turn-policy1/turnserver.conf",
               "--property=UnsetEnvironment=FOURPLAY_WAN_TURN_SECRET",
               "--setenv=FOURPLAY_WAN_TURN_CREDENTIAL=4play-turn.conf",
               "--setenv=FOURPLAY_WAN_TURN_HOST=" + local, "--setenv=FOURPLAY_WAN_PUBLIC_IP=" + public, "--"]
    if service == SERVICES[0]: return command + [str(ROOT / "target/release/control-plane-server"), "--check-wan-turn-credential"]
    return command + ["/usr/bin/python3", str(ROOT / "tools/check-wan-turn-credential.py")]


def check_relay_secret(local, public):
    spec = importlib.util.spec_from_file_location("credential_relay_probe", Path(__file__).with_name("start-local-only-turn.py"))
    helper = importlib.util.module_from_spec(spec); spec.loader.exec_module(helper)
    pid = int(run(["/usr/bin/systemctl", "show", "4play-turn.service", "--property=MainPID", "--value"], capture_output=True, text=True).stdout.strip())
    helper.probe(local, public, stage.existing_secret(installer.bundle.DESTINATION), pid)


def users_and_units():
    import pwd
    users = {}
    for service in SERVICES:
        unit = UNITS / service
        stage.protected_ancestors(unit.parent); stage.check_metadata(unit.lstat())
        for prop, expected in (("FragmentPath", str(unit)), ("DropInPaths", None), ("NeedDaemonReload", "no")):
            value = run(["/usr/bin/systemctl", "show", service, "--property=" + prop, "--value"], capture_output=True, text=True).stdout.strip()
            if prop == "DropInPaths":
                if value not in ("", str(UNITS / (service + ".d") / DROPIN)): raise ValueError("Unexpected application service drop-ins; review before credential installation")
            elif value != expected: raise ValueError("Unexpected application unit fragment")
        user = run(["/usr/bin/systemctl", "show", service, "--property=User", "--value"], capture_output=True, text=True).stdout.strip()
        if not user or pwd.getpwnam(user).pw_uid == 0: raise ValueError("Application services need non-root identities")
        users[service] = user
    return users


def preflight(files):
    allowed = {SETTINGS, *(UNITS / (name + ".d") for name in SERVICES)}
    for path, (data, mode) in files.items():
        for parent in reversed(path.parents):
            if parent.exists() or parent.is_symlink():
                stage.check_metadata(parent.lstat(), directory=True, private=parent == SETTINGS)
            elif parent not in allowed: raise ValueError("Unexpected missing parent directory")
        if path.exists() or path.is_symlink():
            stage.check_metadata(path.lstat(), private=mode == 0o600)
            if path.stat().st_size > 65536 or path.read_bytes() != data: raise ValueError("Conflicting media credential configuration; refusing overwrite")


def install(files):
    created, directories = [], []
    preflight(files)
    try:
        for path, (data, mode) in files.items():
            if not path.parent.exists():
                path.parent.mkdir(mode=0o700 if path.parent == SETTINGS else 0o755)
                metadata = path.parent.lstat()
                directories.append((path.parent, metadata.st_dev, metadata.st_ino))
            if path.exists(): continue
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
            with os.fdopen(descriptor, "wb") as stream:
                metadata = os.fstat(stream.fileno()); created.append((path, metadata.st_dev, metadata.st_ino))
                stream.write(data); stream.flush(); os.fsync(stream.fileno()); os.fchmod(stream.fileno(), mode)
        run(["/usr/bin/systemd-analyze", "verify", *(str(UNITS / name) for name in SERVICES)])
        run(["/usr/bin/systemctl", "daemon-reload"])
        preflight(files)
    except Exception:
        for path, device, inode in reversed(created):
            metadata = path.lstat()
            if (metadata.st_dev, metadata.st_ino) != (device, inode) or not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
                raise RuntimeError("Created media configuration changed during rollback; refusing removal") from None
            path.unlink()
        for path, device, inode in reversed(directories):
            metadata = path.lstat()
            if (metadata.st_dev, metadata.st_ino) != (device, inode) or not stat.S_ISDIR(metadata.st_mode):
                raise RuntimeError("Created media directory changed during rollback; refusing removal") from None
            path.rmdir()  # Empty, exact directories we created only.
        if created: run(["/usr/bin/systemctl", "daemon-reload"])
        raise
    return len(created)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true"); parser.add_argument("--describe", action="store_true")
    args = parser.parse_args()
    if args.apply and args.describe: parser.error("--describe cannot be combined with --apply")
    if args.describe:
        print("Plan only: verify guarded TURN and bundle; test systemd credential readers; install secret-free environment fragments/drop-ins; daemon-reload only.")
        print("No application restart, API-token rotation, public access or firewall/router/nginx change.")
        return
    if sys.platform != "linux" or os.geteuid() != 0: parser.error("Run with sudo on Linux")
    import pwd
    installer.verify_bundle(pwd.getpwnam("turnserver").pw_uid)
    for name in ("4play-turn.service", "4play-turn-egress.service"):
        if run(["/usr/bin/systemctl", "is-active", name], capture_output=True, text=True).stdout.strip() != "active":
            raise ValueError("Host-only TURN must be commissioned first")
    tables = json.loads(run(["/usr/sbin/nft", "-j", "list", "tables"], capture_output=True, text=True).stdout)
    names = {item.get("table", {}).get("name") for item in tables.get("nftables", []) if item.get("table", {}).get("family") == "inet"}
    if not {"fourplay_turn", "fourplay_turn_local_only"} <= names: raise ValueError("Dedicated TURN restrictions are missing")
    encoded_receipt = stage.read_private(installer.bundle.DESTINATION / "deployment-receipt.json")
    receipt = json.loads(encoded_receipt)
    files = contents(receipt["local_ip"], receipt["public_ip"])
    users = users_and_units(); preflight(files)
    if not args.apply:
        print("Guarded TURN, bundle and target preflight passed; no configuration written. --apply runs transient credential checks before installation.")
        return
    check_relay_secret(receipt["local_ip"], receipt["public_ip"])
    for service, user in users.items(): run(credential_command(service, user, receipt["local_ip"], receipt["public_ip"]))
    installer.verify_bundle(pwd.getpwnam("turnserver").pw_uid)
    if stage.read_private(installer.bundle.DESTINATION / "deployment-receipt.json") != encoded_receipt:
        raise ValueError("TURN bundle changed during credential checks")
    print(f"Installed {install(files)} new secret-free media configuration file(s); existing matching files retained.")
    print("Systemd credential delivery configured, not yet live in app processes. No services restarted; public access remains blocked.")


if __name__ == "__main__": main()
