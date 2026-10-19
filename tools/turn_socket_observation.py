"""Test-only UDP observations scoped to a specific process's open descriptors.

/proc/PID/net/udp describes the whole network namespace, NOT only PID's sockets.
Join its inode column to /proc/PID/fd before attributing an allocation to coturn.
"""
import os
from pathlib import Path


def owned_udp_sockets(pid, start, end):
    if type(pid) is not int or pid < 1 or not 1 <= start <= end <= 65535:
        raise ValueError("Expected a process ID and bounded UDP port interval")
    inodes = set()
    for fd in Path(f"/proc/{pid}/fd").iterdir():
        try:
            target = os.readlink(fd)
        except FileNotFoundError:
            continue  # A descriptor can close while its directory is enumerated.
        if target.startswith("socket:[") and target.endswith("]"):
            inodes.add(target[8:-1])
    sockets = {}
    for name in ("udp", "udp6"):
        for line in Path(f"/proc/{pid}/net/{name}").read_text().splitlines()[1:]:
            fields = line.split()
            address, port = fields[1].rsplit(":", 1)
            port = int(port, 16)
            if fields[9] in inodes and start <= port <= end:
                sockets[fields[9]] = (name, address, port)
    return sockets
