#!/usr/bin/env python3
"""Commission TURN on this host only; never open public/remote access.

Loads two dedicated nft tables (temporary ingress guard + service UID egress),
starts the verified relay without enabling it, and tests authenticated payloads.
On failure stop the relay and RETAIN restrictions fail-closed for review.
"""
import argparse
import errno
import importlib.util
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
from turn_socket_observation import owned_udp_sockets
from turn_local_guard import rules


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


installer = load("local_turn_installer", Path(__file__).with_name("install-tested-turn-units.py"))


def run(args, **kwargs):
    return subprocess.run(args, check=True, timeout=30, **kwargs)


def wait_for(predicate, message, seconds=8):
    deadline = time.monotonic() + seconds
    while not predicate():
        if time.monotonic() >= deadline: raise RuntimeError(message)
        time.sleep(.05)


def refuse_existing_tables(document):
    for item in document.get("nftables", []):
        table = item.get("table", {})
        if table.get("family") == "inet" and table.get("name") in ("fourplay_turn", "fourplay_turn_local_only"):
            raise ValueError("A managed TURN table already exists; refusing automatic replacement/removal")


def check_loaded_units():
    for name in installer.NAMES:
        path = installer.TARGET / name
        if not path.is_file() or path.is_symlink(): raise ValueError("Install the verified TURN units first")
        for prop, expected in (("FragmentPath", str(path)), ("DropInPaths", ""), ("NeedDaemonReload", "no")):
            result = run(["/usr/bin/systemctl", "show", name, "--property=" + prop, "--value"], capture_output=True, text=True)
            if result.stdout.strip() != expected: raise ValueError("Loaded TURN unit differs, has overrides or needs daemon-reload")


def check_uid_unused(uid):
    for path in Path("/proc").glob("[0-9]*/status"):
        try: text = path.read_text()
        except (FileNotFoundError, ProcessLookupError): continue
        row = next((line for line in text.splitlines() if line.startswith("Uid:")), "")
        if row and uid in map(int, row.split()[1:]):
            raise ValueError("Dedicated TURN UID already has a process; refusing to apply policy to unknown activity")


def check_relay_pool_unused():
    for name in ("udp", "udp6"):
        for row in Path(f"/proc/net/{name}").read_text().splitlines()[1:]:
            port = int(row.split()[1].rsplit(":", 1)[1], 16)
            if 52000 <= port <= 52199:
                raise ValueError("Reserved UDP relay interval is in use; refusing to affect unrelated sockets")


def probe(host, public, secret, pid):
    protocol = load("local_turn_protocol", ROOT / "tools/test-coturn-loopback.py")
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as echo:
        for _ in range(64):
            try: echo.bind((host, 50000 + secrets.randbelow(1000))); break
            except OSError as error:
                if error.errno != errno.EADDRINUSE: raise
        else: raise RuntimeError("No unused publisher-range probe port available")
        echo.settimeout(3)
        for tcp in (False, True):
            client = protocol.Client(3478, tcp=tcp, host=host)
            allocated = False
            try:
                kind, response = client.allocate(*protocol.credentials(secret, int(time.time()) + 120))
                if kind != 0x0103: raise RuntimeError("Local authenticated TURN allocation failed")
                allocated = True
                address, port = protocol.decoded_address(response[0x0016])
                if address != public or not 52000 <= port <= 52199: raise RuntimeError("Unexpected relay address/range")
                observed = set(owned_udp_sockets(pid, port, port))
                if not observed: raise RuntimeError("Probe allocation has no observed native relay socket")
                kind, _ = client.exchange(8, [(0x0012, protocol.xor_address(host, echo.getsockname()[1]))], True)
                if kind != 0x0108: raise RuntimeError("Probe permission failed")
                packet, _ = protocol.packet(0x0016, [(0x0012, protocol.xor_address(host, echo.getsockname()[1])), (0x0013, b"4play-local-commission")])
                client.socket.sendall(packet)
                payload, source = echo.recvfrom(100)
                if payload != b"4play-local-commission" or source != (host, port): raise RuntimeError("Probe payload/source mismatch")
                echo.sendto(payload, source)
                kind, response = protocol.parse(client.receive())
                if kind != 0x0017 or response[0x0013] != payload: raise RuntimeError("Relayed probe response mismatch")
            finally:
                client.close(allocated)
            wait_for(lambda: not observed.intersection(owned_udp_sockets(pid, 52000, 52199)), "Local probe allocation did not release")
            print(f"PASS: local authenticated {'TCP' if tcp else 'UDP'} TURN payload and native allocation release")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--describe", action="store_true")
    args = parser.parse_args()
    if args.describe and args.apply: parser.error("--describe cannot be combined with --apply")
    if args.describe:
        print("Plan only: isolated ingress test; host-only TURN guard; dedicated UID output policy; start disabled relay; local auth/payload checks.")
        print("No UFW, router, nginx, app environment or public access changes. Failure stops relay but retains restrictions.")
        return
    if sys.platform != "linux" or os.geteuid() != 0: parser.error("Run with sudo on the Linux host")
    if os.readlink("/proc/self/ns/net") != os.readlink("/proc/1/ns/net"):
        raise RuntimeError("Commissioning must target the host network namespace")
    import pwd
    account = pwd.getpwnam("turnserver")
    if account.pw_uid == 0 or os.getgrouplist(account.pw_name, account.pw_gid) != [account.pw_gid]:
        raise ValueError("Dedicated non-root account without supplemental groups required")
    files = installer.verify_bundle(account.pw_uid)
    installer.check_targets(files); installer.check_inactive(); check_loaded_units(); check_uid_unused(account.pw_uid); check_relay_pool_unused()
    document = json.loads(run(["/usr/sbin/nft", "-j", "list", "ruleset"], capture_output=True, text=True).stdout)
    refuse_existing_tables(document)
    # Refuse a listener conflict before loading any rules.
    receipt = json.loads(installer.stage.read_private(installer.bundle.DESTINATION / "deployment-receipt.json"))
    host, public = receipt["local_ip"], receipt["public_ip"]
    for kind in (socket.SOCK_DGRAM, socket.SOCK_STREAM):
        with socket.socket(socket.AF_INET, kind) as listener: listener.bind((host, 3478))
    run(["/usr/sbin/nft", "--check", "-f", "-"], input=rules(), text=True)
    if not args.apply:
        print("Preflight passed. --apply loads dedicated restrictions and starts host-only TURN; no changes made.")
        return
    run([sys.executable, str(ROOT / "tools/test-turn-local-guard.py")])
    # Recheck all protected inputs after the isolated test, before host mutation.
    if installer.verify_bundle(account.pw_uid) != files: raise ValueError("Bundle changed during validation")
    installer.check_targets(files); installer.check_inactive(); check_loaded_units(); check_uid_unused(account.pw_uid); check_relay_pool_unused()
    refuse_existing_tables(json.loads(run(["/usr/sbin/nft", "-j", "list", "ruleset"], capture_output=True, text=True).stdout))
    attempted = False
    try:
        run(["/usr/sbin/nft", "-f", "-"], input=rules(), text=True)
        attempted = True
        run(["/usr/bin/systemctl", "start", "4play-turn.service"])
        def listening():
            try:
                with socket.create_connection((host, 3478), timeout=.2): return True
            except OSError: return False
        wait_for(listening, "TURN listener failed to start")
        pid = int(run(["/usr/bin/systemctl", "show", "4play-turn.service", "--property=MainPID", "--value"], capture_output=True, text=True).stdout.strip())
        fixture = load("host_turn_identity", ROOT / "tools/test-private-turn-service.py")
        fixture.check_process(pid, Path("/proc/1/ns/net"), account.pw_uid)
        if Path(os.readlink(f"/proc/{pid}/exe")) != installer.bundle.candidate.BINARY:
            raise RuntimeError("TURN is not running the verified candidate binary")
        secret = installer.stage.existing_secret(installer.bundle.DESTINATION)
        probe(host, public, secret, pid)
    except Exception:
        try:
            if attempted: run(["/usr/bin/systemctl", "stop", "4play-turn.service"])
        finally:
            print("Commissioning failed; any installed restrictions are retained fail-closed. Do not remove tables or retry blindly; review the output.", file=sys.stderr)
        raise
    print("PASS: verified candidate running host-only, non-root and without capabilities; relay remains disabled at boot")
    print("Dedicated ingress/egress restrictions are active. No public access, UFW/router, nginx or app configuration was enabled.")


if __name__ == "__main__": main()
