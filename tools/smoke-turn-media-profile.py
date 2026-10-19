"""Unprivileged routing/media smoke test in a fresh user/network namespace.

NO outbound policy or systemd confinement is tested here. This catches media
fixture issues before the separate privileged service + firewall acceptance.
No real LAN/public network, production credential or service is used.
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

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/systemd"))
from private_turn_bundle import turn_config


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--parent-net", help=argparse.SUPPRESS)
    parser.add_argument("--turnserver", type=Path, default=Path("/opt/4play-coturn/4.18.0-4play-policy1/turnserver"))
    args = parser.parse_args()
    if not args.inside:
        if os.geteuid() == 0:
            parser.error("Run without sudo")
        subprocess.run(["/usr/bin/unshare", "--user", "--map-root-user", "--net", sys.executable,
                        str(Path(__file__).resolve()), "--inside", "--parent-net", os.readlink("/proc/self/ns/net"),
                        "--turnserver", str(args.turnserver.resolve(strict=True))], check=True)
        return
    mapping = Path("/proc/self/uid_map").read_text().split()
    if not args.parent_net or os.readlink("/proc/self/ns/net") == args.parent_net or len(mapping) != 3 or mapping[0] != "0" or mapping[2] != "1" or int(mapping[1]) == 0:
        raise RuntimeError("Refusing smoke tests outside fresh user/network namespaces")
    subprocess.run(["/usr/sbin/ip", "link", "set", "lo", "up"], check=True)
    subprocess.run(["/usr/sbin/ip", "link", "add", "fixture0", "type", "dummy"], check=True)
    subprocess.run(["/usr/sbin/ip", "link", "set", "fixture0", "up"], check=True)
    for address in ["192.168.20.68", "45.24.46.155"]:
        interface = "fixture0" if address == "192.168.20.68" else "lo"
        subprocess.run(["/usr/sbin/ip", "addr", "add", address + "/32", "dev", interface], check=True)
    spec = importlib.util.spec_from_file_location("media_service_fixture", ROOT / "tools/test-private-turn-service.py")
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    with tempfile.TemporaryDirectory(prefix="4play-media-smoke-") as directory:
        root = Path(directory)
        config = root / "turn.conf"
        key = secrets.token_hex(32)
        text = turn_config("192.168.20.68", "45.24.46.155", key)
        text = text.replace("relay-ip=192.168.20.68\n", "relay-ip=45.24.46.155\n")
        text = text.replace("external-ip=45.24.46.155/192.168.20.68\n", "external-ip=45.24.46.155/45.24.46.155\n")
        text = text.replace("/run/4play-turn/turnserver.pid", str(root / "turn.pid"))
        config.write_text(text + "\nverbose\n")
        config.chmod(0o600)
        with (root / "turn.log").open("w") as log:
            process = subprocess.Popen(["/usr/bin/setpriv", "--no-new-privs", "--bounding-set=-all", str(args.turnserver), "-c", str(config)], stdout=log, stderr=log)
            try:
                def listening():
                    try:
                        with socket.create_connection(("192.168.20.68", 3478), timeout=.2): return True
                    except OSError: return False
                fixture.wait_for(listening, "Smoke-test TURN listener failed to start")
                fixture.media_checks(config, process.pid, parent_net=args.parent_net)
            except Exception:
                lines = (root / "turn.log").read_text(errors="replace").replace(key, "[redacted]").splitlines()
                selected = [line for line in lines if any(word in line.lower() for word in ("error", "warn", "quota"))]
                print("Fixture-only smoke warnings/errors:\n" + "\n".join(selected[-30:]), flush=True)
                raise
            finally:
                process.terminate()
                try: process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=3)
    print("PASS: routing/media smoke only; systemd confinement/firewall validation is still a separate privileged gate")


if __name__ == "__main__": main()
