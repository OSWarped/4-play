"""Non-root diagnosis in a fresh USER + NETWORK namespace, not a service test.

Only this namespace's fixture client sockets carry mark 1 to model unfiltered
non-TURN UIDs; coturn's sockets retain mark 0 and receive the same outbound rules.
Never use this test-only UID/mark adaptation in production firewall rules.
"""
import argparse
import importlib.util
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import tempfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/systemd"))
from private_turn_bundle import egress_rules, turn_config


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--parent-net", help=argparse.SUPPRESS)
    parser.add_argument("--single-recv", action="store_true", help="Diagnostic only: disable coturn's optional recvmmsg optimization")
    parser.add_argument("--turnserver", type=Path, default=Path("/opt/4play-coturn/4.18.0/turnserver"), help="Explicit installed baseline or private candidate binary")
    args = parser.parse_args()
    if not args.inside:
        if os.geteuid() == 0:
            parser.error("Run as ordinary user, WITHOUT sudo")
        subprocess.run(["/usr/bin/unshare", "--user", "--map-root-user", "--net", sys.executable,
                        str(Path(__file__).resolve()), "--inside", "--parent-net", os.readlink("/proc/self/ns/net"),
                        "--turnserver", str(args.turnserver.resolve(strict=True)),
                        *(["--single-recv"] if args.single_recv else [])], check=True)
        return
    mapping = Path("/proc/self/uid_map").read_text().split()
    if not args.parent_net or os.readlink("/proc/self/ns/net") == args.parent_net or len(mapping) != 3 or mapping[0] != "0" or mapping[2] != "1" or int(mapping[1]) == 0:
        raise RuntimeError("Refusing diagnostics outside fresh user/network namespaces")
    subprocess.run(["/usr/sbin/ip", "link", "set", "lo", "up"], check=True)
    for address in ["192.168.20.68", "192.168.20.1", "45.24.46.155", "8.8.8.8"]:
        subprocess.run(["/usr/sbin/ip", "addr", "add", address + "/32", "dev", "lo"], check=True)
    spec = importlib.util.spec_from_file_location("turn_release_fixture", ROOT / "tools/test-private-turn-service.py")
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    real_socket = socket.socket
    def client_socket(*args, **kwargs):
        sock = real_socket(*args, **kwargs)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_MARK, 1)
        return sock
    with tempfile.TemporaryDirectory(prefix="4play-release-repro-") as directory:
        root = Path(directory)
        rules = egress_rules(1, "192.168.20.68", "45.24.46.155")
        rules = rules.replace("meta skuid != 1 return", "meta mark 1 return\n    meta skuid != 0 return")
        rule_file = root / "fixture.nft"
        rule_file.write_text(rules)
        subprocess.run(["/usr/sbin/nft", "-f", str(rule_file)], check=True)
        for denied_send in (False, True):
            config = root / "fixture.conf"
            key = secrets.token_hex(32)
            text = turn_config("192.168.20.68", "45.24.46.155", key)
            text = text.replace("/run/4play-turn/turnserver.pid", str(root / "turn.pid"))
            config.write_text(text + "\nVerbose\n" + ("udp-recvmmsg=false\n" if args.single_recv else ""))
            config.chmod(0o600)
            log = root / "fixture.log"
            with log.open("w") as output:
                process = subprocess.Popen(["/usr/bin/setpriv", "--no-new-privs", "--bounding-set=-all",
                    str(args.turnserver), "-c", str(config)], stdout=output, stderr=output)
                try:
                    print(f"CASE: forbidden-port send={denied_send}", flush=True)
                    with patch.object(socket, "socket", client_socket):
                        fixture.protocol_checks(config, process.pid, probe_private_web=denied_send, parent_net=args.parent_net)
                except Exception:
                    lines = log.read_text(errors="replace").replace(key, "[redacted]").splitlines()
                    lines = [line for line in lines if "timer_event_handler: timeout" not in line or "allocation_timeout_handler" in line]
                    print("Fixture-only coturn log (last 45 non-periodic lines):", flush=True)
                    print("\n".join(lines[-45:]), flush=True)
                    raise
                finally:
                    process.terminate()
                    try: process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=3)


if __name__ == "__main__": main()
