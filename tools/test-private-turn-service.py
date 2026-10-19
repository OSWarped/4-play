"""Validate real coturn in a transient systemd sandbox + private network namespace.

Requires sudo. Never installs permanent units, opens host ports, loads host nft
rules, or edits staging/app/nginx credentials. Uses a fresh throwaway test key.
Full browser media and WAN/NAT acceptance remain separate gates.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from turn_socket_observation import owned_udp_sockets

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/systemd"))
from private_turn_bundle import service_units, turn_config


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def service_properties(unit):
    properties = {}
    section = None
    for line in unit.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("["):
            section = line
        elif section == "[Service]":
            key, value = line.split("=", 1)
            if key in properties:
                raise ValueError("Ambiguous service property")
            properties[key] = value
    return properties


def transient_command(unit, net, config, log, binary):
    if not re.fullmatch(r"4play-turn-validation-[a-f0-9]{32}\.service", unit):
        raise ValueError("Invalid isolated test unit name")
    props = service_properties(service_units()["4play-turn.service"])
    props.pop("ExecStart")
    runtime = unit.removesuffix(".service")
    props.update(Type="exec", Restart="no", RuntimeMaxSec="90s", RuntimeDirectory=runtime,
                 NetworkNamespacePath=str(net), LoadCredential=f"turnserver.conf:{config}",
                 StandardOutput=f"append:{log}", StandardError=f"append:{log}")
    command = ["/usr/bin/systemd-run", "--quiet", "--collect", "--unit=" + unit]
    command.extend(f"--property={key}={value}" for key, value in props.items())
    command += ["--", str(binary), "-c", f"/run/credentials/{unit}/turnserver.conf"]
    return command


def wait_for(predicate, message, seconds=5):
    deadline = time.monotonic() + seconds
    while not predicate():
        if time.monotonic() >= deadline:
            raise RuntimeError(message)
        time.sleep(.05)


def check_process(pid, namespace, uid):
    status = dict(line.split(":", 1) for line in Path(f"/proc/{pid}/status").read_text().splitlines() if ":" in line)
    if any(int(value) != uid for value in status["Uid"].split()):
        raise RuntimeError("TURN is not running under its dedicated non-root UID")
    if any(int(status[name].strip(), 16) != 0 for name in ("CapEff", "CapPrm", "CapInh", "CapBnd", "CapAmb")) or status["NoNewPrivs"].strip() != "1":
        raise RuntimeError("TURN capability/no-new-privileges confinement is missing")
    if os.readlink(f"/proc/{pid}/ns/net") != os.readlink(namespace):
        raise RuntimeError("TURN did not join the isolated network namespace")
    if os.readlink(f"/proc/{pid}/ns/mnt") == os.readlink("/proc/1/ns/mnt"):
        raise RuntimeError("TURN filesystem sandbox was not created")


def protocol_checks(config, pid, probe_private_web=True, parent_net=None):
    if os.readlink("/proc/self/ns/net") == (parent_net or os.readlink("/proc/1/ns/net")):
        raise RuntimeError("Refusing fixture protocol checks in the host network namespace")
    if os.readlink(f"/proc/{pid}/ns/net") != os.readlink("/proc/self/ns/net"):
        raise RuntimeError("TURN process is outside this fixture namespace")
    text = config.read_text()
    def value(name):
        result = re.findall(r"^" + re.escape(name) + r"=(.+)$", text, re.MULTILINE)
        if len(result) != 1:
            raise ValueError("Ambiguous fixture configuration")
        return result[0]
    host = value("listening-ip")
    public = value("external-ip").split("/")[0]
    secret = value("static-auth-secret")
    protocol = load("turn_service_protocol", ROOT / "tools/test-coturn-loopback.py")
    def listening():
        try:
            with socket.create_connection((host, 3478), timeout=.2):
                return True
        except OSError:
            return False
    wait_for(listening, "Confined TURN listener did not become ready")
    tracked = set()
    # Root fixture sockets model a public UDP endpoint and the private runtime.
    # They exist only in this namespace, not on the actual LAN/public network.
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as echo, socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as runtime, socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as forbidden:
        echo.bind(("8.8.8.8", 45000))
        echo.settimeout(3)
        runtime.bind((host, 50001))
        runtime.settimeout(3)
        forbidden.bind((host, 8080))
        forbidden.settimeout(.3)
        for tcp in (False, True):
            client = protocol.Client(3478, tcp=tcp, host=host)
            allocated = False
            try:
                kind, response = client.allocate(*protocol.credentials(secret, int(time.time()) + 120))
                if kind != 0x0103:
                    raise AssertionError("Valid fixture TURN credentials were rejected")
                allocated = True
                address, port = protocol.decoded_address(response[0x0016])
                assert address == public and 52000 <= port <= 52199
                allocation_sockets = set(owned_udp_sockets(pid, port, port))
                if not allocation_sockets:
                    raise AssertionError("The advertised allocation has no observable coturn-owned relay socket")
                tracked.update(allocation_sockets)
                def permission(ip, port):
                    return client.exchange(8, [(0x0012, protocol.xor_address(ip, port))], True)
                def send(ip, port):
                    packet, _ = protocol.packet(0x0016, [(0x0012, protocol.xor_address(ip, port)), (0x0013, b"confined-relay-check")])
                    client.socket.sendall(packet)
                kind, _ = permission("8.8.8.8", 45000)
                assert kind == 0x0108
                send("8.8.8.8", 45000)
                data, peer = echo.recvfrom(100)
                assert data == b"confined-relay-check" and 52000 <= peer[1] <= 52199
                echo.sendto(data, peer)
                kind, response = protocol.parse(client.receive())
                assert kind == 0x0017 and response[0x0013] == data
                kind, _ = permission(host, 50001)
                assert kind == 0x0108
                send(host, 50001)
                data, peer = runtime.recvfrom(100)
                assert data == b"confined-relay-check"
                runtime.sendto(data, peer)
                kind, response = protocol.parse(client.receive())
                assert kind == 0x0017 and response[0x0013] == data
                kind, response = permission("192.168.20.1", 50000)
                assert kind == 0x0118 and response[0x0009][2:4] == b"\x04\x03", "Unrelated private peer permission was allowed"
                # The IP ACL intentionally allows the runtime IP. The kernel
                # egress policy, not that IP-only ACL, must protect its web port.
                if probe_private_web:
                    kind, _ = permission(host, 8080)
                    assert kind == 0x0108
                    send(host, 8080)
                    try:
                        forbidden.recv(100)
                    except socket.timeout:
                        pass
                    else:
                        raise AssertionError("Authenticated relay reached the private web port")
                    send("8.8.8.8", 45000)
                    try:
                        data, peer = echo.recvfrom(100)
                    except socket.timeout:
                        raise AssertionError("Valid relay traffic stopped after a firewall-denied datagram") from None
                    assert data == b"confined-relay-check"
                    echo.sendto(data, peer)
                    kind, response = protocol.parse(client.receive())
                    assert kind == 0x0017 and response[0x0013] == data
            finally:
                client.close(allocated)
            if allocated:
                def released():
                    return not allocation_sockets.intersection(owned_udp_sockets(pid, 52000, 52199))
                try:
                    wait_for(released, "Confined relay allocation socket did not release", seconds=10)
                except RuntimeError:
                    remaining = owned_udp_sockets(pid, 52000, 52199)
                    ports = sorted({remaining[inode][2] for inode in allocation_sockets.intersection(remaining)})
                    raise RuntimeError(f"Coturn still owns the observed allocation socket(s) on UDP port(s) {ports}") from None
            print(f"PASS: confined {'TCP' if tcp else 'UDP'} TURN authentication, relay payload, private-service denial, allocation release")
        for label, credential in [("expired", protocol.credentials(secret, int(time.time()) - 60)),
                                  ("wrong-secret", protocol.credentials(secrets.token_hex(32), int(time.time()) + 120))]:
            client = protocol.Client(3478, host=host)
            try:
                kind, response = client.allocate(*credential)
                assert kind == 0x0113 and response[0x0009][2:4] == b"\x04\x01"
            finally:
                client.close()
            print(f"PASS: confined relay rejects {label} credentials")
    wait_for(lambda: not tracked.intersection(owned_udp_sockets(pid, 52000, 52199)),
             "Confined observed allocations did not release", seconds=10)
    print("PASS: observed coturn allocation sockets released while the confined service is still running")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--protocol-config", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--turn-pid", type=int, help=argparse.SUPPRESS)
    parser.add_argument("--policy-candidate", action="store_true", help="Test the separately installed, verified local policy-error patch candidate")
    args = parser.parse_args()
    if sys.platform != "linux" or os.geteuid() != 0:
        parser.error("Run with sudo on the reference Linux host")
    if args.protocol_config:
        if not args.turn_pid or args.turn_pid < 1:
            parser.error("Fixture process ID required")
        protocol_checks(args.protocol_config, args.turn_pid)
        return
    stage = load("private_turn_stage", ROOT / "deploy/systemd/stage-private-turn.py")
    directory = stage.DESTINATION
    stage.protected_ancestors(directory)
    secret = stage.existing_secret(directory)
    receipt = json.loads(stage.read_private(directory / "stage-receipt.json"))
    stage.verify_binary(receipt["binary_sha256"])
    binary = stage.BINARY
    if args.policy_candidate:
        local_candidate = load("installed_policy_candidate", ROOT / "deploy/systemd/install-coturn-policy-candidate.py")
        binary = local_candidate.verify_installed()
    import pwd
    account = pwd.getpwnam("turnserver")
    expected = stage.contents(account.pw_uid, receipt["local_ip"], receipt["public_ip"], secret, receipt["binary_sha256"])
    if {path.name for path in directory.iterdir()} != set(expected):
        raise RuntimeError("Unexpected private staging files")
    for name, data in expected.items():
        if stage.read_private(directory / name) != data:
            raise RuntimeError("Private TURN staging differs from reviewed templates")
    if receipt["local_ip"] != "192.168.20.68" or receipt["public_ip"] != "45.24.46.155":
        raise RuntimeError("This isolated fixture currently supports the reference address pair only")
    unit = "4play-turn-validation-" + uuid.uuid4().hex + ".service"
    keeper = subprocess.Popen(["/usr/bin/unshare", "--net", sys.executable, "-c",
                               "import sys; print('ready',flush=True); sys.stdin.buffer.read()"],
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    attempted = False
    try:
        if keeper.stdout.readline() != b"ready\n":
            raise RuntimeError("Private namespace creation failed")
        net = Path(f"/proc/{keeper.pid}/ns/net")
        if os.readlink(net) == os.readlink("/proc/1/ns/net"):
            raise RuntimeError("Refusing service tests in the host network namespace")
        enter = ["/usr/bin/nsenter", f"--net={net}"]
        subprocess.run(enter + [sys.executable, str(ROOT / "tools/test-private-turn-egress.py"), "--inside"], check=True, timeout=60)
        with tempfile.TemporaryDirectory(prefix="4play-confined-turn-") as temporary:
            config = Path(temporary) / "turnserver.conf"
            text = turn_config(receipt["local_ip"], receipt["public_ip"], secrets.token_hex(32))
            text = text.replace("/run/4play-turn/turnserver.pid", f"/run/{unit.removesuffix('.service')}/turnserver.pid")
            stage.write_exclusive(config, text.encode())
            command = transient_command(unit, net, config, Path(temporary) / "service.log", binary)
            attempted = True
            subprocess.run(command, check=True, timeout=30)
            result = subprocess.run(["/usr/bin/systemctl", "show", unit, "--property=MainPID", "--value"], capture_output=True, text=True, check=True)
            pid = int(result.stdout.strip())
            check_process(pid, net, account.pw_uid)
            print("PASS: actual coturn runs non-root with no capabilities, no-new-privileges, private network and mount namespaces")
            subprocess.run(enter + [sys.executable, str(Path(__file__).resolve()), "--protocol-config", str(config), "--turn-pid", str(pid)], check=True, timeout=45)
    finally:
        # Stop only the randomly named transient validation service, never any
        # production service. RuntimeMaxSec is a second, independent failsafe.
        try:
            if attempted:
                stopped = subprocess.run(["/usr/bin/systemctl", "stop", unit], capture_output=True, text=True, check=False, timeout=30)
                if stopped.returncode:
                    state = subprocess.run(["/usr/bin/systemctl", "is-active", unit], capture_output=True, text=True, check=False, timeout=5)
                    if state.stdout.strip() not in ("inactive", "unknown", "failed"):
                        raise RuntimeError("Transient validation service could not be stopped; its 90-second runtime limit remains in effect")
        finally:
            keeper.stdin.close()
            try:
                keeper.wait(timeout=5)
            except subprocess.TimeoutExpired:
                keeper.kill()
                keeper.wait(timeout=5)
            keeper.stdout.close()
    print("PASS: transient relay stopped and namespace released; permanent services, staging credentials and host firewall unchanged")


if __name__ == "__main__": main()
