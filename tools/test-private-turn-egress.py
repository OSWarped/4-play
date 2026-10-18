"""Root-only kernel firewall tests in a NEW network namespace, never the host.

No services are installed/started. Uses only fixture addresses inside the private
namespace; UFW, host routes, interfaces and public exposure remain unchanged.
"""
import argparse
import importlib.util
import os
from pathlib import Path
import pwd
import socket
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location("turn_bundle", Path(__file__).resolve().parents[1] / "deploy/systemd/private_turn_bundle.py")
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)

WORKER = """import errno,socket,sys
host,port,source,family = sys.argv[1:]
s=socket.socket(socket.AF_INET6 if family=='6' else socket.AF_INET,socket.SOCK_DGRAM)
s.bind(('::1' if family=='6' else '192.168.20.68', int(source)))
try:
    try:
        s.sendto(b'4play-egress-fixture',(host,int(port)))
    except OSError as error:
        if error.errno not in (errno.EPERM, errno.EACCES):
            raise
        sys.exit(77)
finally:
    s.close()
"""


def validate_udp_result(returncode, data, allowed):
    # Local OUTPUT drops can fail sendto immediately instead of silently timing
    # out. Accept ONLY the worker's explicit permission-denied sentinel for a
    # forbidden packet, never a bind failure or another subprocess error.
    if returncode not in (0, 77):
        raise AssertionError(f"UDP worker failed unexpectedly: exit {returncode}")
    if allowed:
        if returncode != 0 or data != b"4play-egress-fixture":
            raise AssertionError("Allowed UDP packet was blocked or did not arrive")
    elif data is not None:
        raise AssertionError("Forbidden UDP packet reached the receiver")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not hasattr(os, "geteuid") or os.geteuid() != 0 or sys.platform != "linux":
        parser.error("Run with sudo on the Linux reference host; testing requires a private network namespace")
    if not args.inside:
        subprocess.run(["/usr/bin/unshare", "--net", sys.executable, str(Path(__file__).resolve()), "--inside"], check=True)
        return
    if os.readlink("/proc/self/ns/net") == os.readlink("/proc/1/ns/net"):
        raise RuntimeError("Refusing firewall/address changes in the host network namespace")
    account = pwd.getpwnam("turnserver")
    subprocess.run(["/usr/sbin/ip", "link", "set", "lo", "up"], check=True)
    for address in ["192.168.20.68", "192.168.20.1", "45.24.46.155", "8.8.8.8"]:
        subprocess.run(["/usr/sbin/ip", "addr", "add", address + "/32", "dev", "lo"], check=True)
    with tempfile.TemporaryDirectory(prefix="4play-egress-namespace-") as directory:
        rules = Path(directory) / "egress.nft"
        rules.write_text(bundle.egress_rules(account.pw_uid, "192.168.20.68", "45.24.46.155"))
        subprocess.run(["/usr/sbin/nft", "--check", "--file", str(rules)], check=True)
        subprocess.run(["/usr/sbin/nft", "--file", str(rules)], check=True)
        units = []
        for name, text in bundle.service_units().items():
            unit = Path(directory) / name
            unit.write_text(text)
            units.append(str(unit))
        subprocess.run(["/usr/bin/systemd-analyze", "verify", *units], check=True)
        def udp(host, port, source, allowed, uid=None):
            family = socket.AF_INET6 if ":" in host else socket.AF_INET
            with socket.socket(family, socket.SOCK_DGRAM) as receiver:
                receiver.bind((host, port))
                receiver.settimeout(.2)
                result = subprocess.run([sys.executable, "-c", WORKER, host, str(port), str(source),
                                "6" if family == socket.AF_INET6 else "4"],
                               user=account.pw_uid if uid is None else uid,
                               group=account.pw_gid, extra_groups=[], cwd="/", check=False, timeout=5)
                try:
                    data = receiver.recv(100)
                except socket.timeout:
                    data = None
                try:
                    validate_udp_result(result.returncode, data, allowed)
                except AssertionError as error:
                    raise AssertionError(f"Unexpected egress result for {host}:{port} source={source}: {error}") from error
        for host, port in [("192.168.20.68", 50000), ("192.168.20.68", 50999),
                           ("45.24.46.155", 50000), ("45.24.46.155", 52000), ("45.24.46.155", 52199),
                           ("8.8.8.8", 45000)]:
            udp(host, port, 52010, True)
        print("PASS: relay UID can reach intended publisher ports and public UDP peers")
        for host, port, source in [("192.168.20.68", 8080, 52010), ("192.168.20.68", 22, 52010),
                                   ("192.168.20.68", 49999, 52010), ("192.168.20.68", 51000, 52010),
                                   ("192.168.20.1", 50000, 52010), ("127.0.0.1", 8080, 52010),
                                   ("45.24.46.155", 8080, 52010), ("45.24.46.155", 52200, 51999),
                                   ("8.8.8.8", 45000, 51999), ("8.8.8.8", 45000, 52200),
                                   ("192.168.20.1", 8080, 3478),
                                   ("::1", 8080, 52010)]:
            udp(host, port, source, False)
        print("PASS: unrelated LAN/loopback services, IPv6 and non-relay public egress blocked")
        udp("192.168.20.68", 8080, 52010, True, uid=0)
        print("PASS: other UIDs are unaffected")
        # Listener responses must still work. Only existing conntrack replies,
        # not arbitrary packets bound to source 3478, receive this exception.
        reply_worker = """import socket
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM)
s.bind(('192.168.20.68',3478))
s.settimeout(3)
print('ready',flush=True)
data,peer=s.recvfrom(100)
s.sendto(data,peer)
"""
        process = subprocess.Popen([sys.executable, "-c", reply_worker], user=account.pw_uid,
                                   group=account.pw_gid, extra_groups=[], cwd="/", stdout=subprocess.PIPE)
        try:
            assert process.stdout.readline() == b"ready\n"
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as caller:
                caller.bind(("192.168.20.1", 0))
                caller.settimeout(2)
                caller.sendto(b"reply-check", ("192.168.20.68", 3478))
                assert caller.recv(100) == b"reply-check"
            assert process.wait(timeout=3) == 0
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=3)
            process.stdout.close()
        print("PASS: established UDP listener responses work without allowing arbitrary source-3478 egress")
    print("PASS: private namespace exits; host firewall, routes and services were not changed")


if __name__ == "__main__": main()
