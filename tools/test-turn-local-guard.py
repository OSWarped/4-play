"""Real ingress packets in disposable namespaces; no host firewall changes."""
import argparse
import os
from pathlib import Path
import socket
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/systemd"))
from turn_local_guard import rules


def command(args, **kwargs):
    return subprocess.run(args, check=True, timeout=10, **kwargs)


def inside(parent_net):
    current = os.readlink("/proc/self/ns/net")
    if not parent_net or current == parent_net:
        raise RuntimeError("Refusing ingress fixture in host namespace")
    try:
        if current == os.readlink("/proc/1/ns/net"):
            raise RuntimeError("Refusing ingress fixture in host namespace")
    except PermissionError:
        mapping = Path("/proc/self/uid_map").read_text().split()
        if len(mapping) != 3 or mapping[0] != "0" or mapping[2] != "1" or int(mapping[1]) == 0:
            raise RuntimeError("Cannot verify disposable user namespace") from None
    keeper = subprocess.Popen(["/usr/bin/unshare", "--net", sys.executable, "-c",
                               "import sys; print('ready',flush=True); sys.stdin.buffer.read()"], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    try:
        if keeper.stdout.readline() != b"ready\n": raise RuntimeError("Peer namespace failed")
        if os.readlink(f"/proc/{keeper.pid}/ns/net") == os.readlink("/proc/self/ns/net"):
            raise RuntimeError("Fixture peer namespace not isolated")
        peer = ["/usr/bin/nsenter", f"--net=/proc/{keeper.pid}/ns/net"]
        ip = ["/usr/sbin/ip"]
        command(ip + ["link", "set", "lo", "up"])
        command(ip + ["link", "add", "guard-host", "type", "veth", "peer", "name", "guard-peer"])
        command(ip + ["link", "set", "guard-peer", "netns", str(keeper.pid)])
        command(ip + ["addr", "add", "198.18.0.1/30", "dev", "guard-host"])
        command(ip + ["link", "set", "guard-host", "up"])
        command(peer + ip + ["link", "set", "lo", "up"])
        command(peer + ip + ["addr", "add", "198.18.0.2/30", "dev", "guard-peer"])
        command(peer + ip + ["link", "set", "guard-peer", "up"])
        command(["/usr/sbin/nft", "-f", "-"], input=rules(), text=True)
        for port, allowed in ((3478, False), (52000, False), (52199, False), (8080, True)):
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as receiver:
                receiver.bind(("0.0.0.0", port)); receiver.settimeout(.3)
                code = "import socket; s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.sendto(b'guard-fixture',('198.18.0.1',int(__import__('sys').argv[1])))"
                command(peer + [sys.executable, "-c", code, str(port)])
                try: payload = receiver.recv(100)
                except socket.timeout:
                    if allowed: raise AssertionError("Unrelated web-port traffic was blocked")
                else:
                    if not allowed or payload != b"guard-fixture": raise AssertionError("Remote TURN ingress was not blocked")
                if port == 3478:
                    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender:
                        sender.sendto(b"local", ("127.0.0.1", port))
                    assert receiver.recv(100) == b"local"
        for port, allowed in ((3478, False), (8080, True)):
            with socket.socket() as listener:
                listener.bind(("0.0.0.0", port)); listener.listen(); listener.settimeout(.3)
                code = "import socket,sys; s=socket.socket(); s.settimeout(.3); result=s.connect_ex(('198.18.0.1',int(sys.argv[1]))); sys.exit(0 if (result==0)==(sys.argv[2]=='yes') else 1)"
                command(peer + [sys.executable, "-c", code, str(port), "yes" if allowed else "no"])
                if allowed:
                    connection, _ = listener.accept(); connection.close()
                else:
                    try: connection, _ = listener.accept()
                    except socket.timeout: pass
                    else:
                        connection.close(); raise AssertionError("Remote TCP TURN ingress was allowed")
        # Ordinary locally initiated UDP must still receive established replies
        # even if the ephemeral source port falls in the TURN relay interval.
        echo_code = "import socket; s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.bind(('198.18.0.2',53000)); s.settimeout(3); print('ready',flush=True); data,addr=s.recvfrom(100); s.sendto(data,addr)"
        echo = subprocess.Popen(peer + [sys.executable, "-c", echo_code], stdout=subprocess.PIPE)
        try:
            if echo.stdout.readline() != b"ready\n": raise RuntimeError("Peer echo failed")
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender:
                sender.bind(("198.18.0.1", 52010)); sender.settimeout(3)
                sender.sendto(b"established", ("198.18.0.2", 53000))
                assert sender.recv(100) == b"established"
            assert echo.wait(timeout=5) == 0
        finally:
            if echo.poll() is None: echo.kill(); echo.wait(timeout=5)
            echo.stdout.close()
        print("PASS: remote UDP/TCP TURN and relay ingress blocked; local access, unrelated web ports and established UDP replies preserved")
    finally:
        keeper.stdin.close()
        try: keeper.wait(timeout=5)
        except subprocess.TimeoutExpired: keeper.kill(); keeper.wait(timeout=5)
        keeper.stdout.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--parent-net", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if sys.platform != "linux" or os.geteuid() != 0: parser.error("Run with sudo on Linux")
    if args.inside: inside(args.parent_net)
    else:
        command(["/usr/bin/unshare", "--net", sys.executable, str(Path(__file__).resolve()), "--inside", "--parent-net", os.readlink("/proc/self/ns/net")])
        print("PASS: namespaces released; host routes, firewall and services unchanged")


if __name__ == "__main__": main()
