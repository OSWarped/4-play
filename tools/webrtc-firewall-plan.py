#!/usr/bin/env python3
"""Print a source-restricted WebRTC firewall plan. Never applies rules."""
import argparse
import ipaddress
import os
from pathlib import Path
import shlex

from webrtc_ice_ports import IceUdpPortRange, DEFAULT_ICE_UDP_PORT_START, DEFAULT_ICE_UDP_PORT_COUNT


KEYS = {"FOURPLAY_CONTROL_PLANE_BIND", "FOURPLAY_WEBRTC_ICE_UDP_PORT_START", "FOURPLAY_WEBRTC_ICE_UDP_PORT_COUNT"}
KEYS.update(f"FOURPLAY_{prefix}_PORT_{suffix}" for prefix in ("INPUT", "MEDIA") for suffix in ("START", "COUNT"))


def read_settings(path, environment):
    settings = {key: value for key, value in environment.items() if key in KEYS}
    if path is None:
        return settings
    # Systemd EnvironmentFile values are data, not executable shell expressions.
    for number, line in enumerate(Path(path).read_text(encoding="utf-8").splitlines(), 1):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        key, separator, raw = stripped.partition("=")
        if not separator or key not in KEYS:
            continue  # Never parse or print bearer tokens or other unrelated values.
        values = shlex.split(raw, comments=True)
        if len(values) != 1:
            raise ValueError(f"invalid literal configuration for {key} on line {number}")
        settings[key] = values[0]
    return settings


def plan(settings, sources):
    ports = IceUdpPortRange(int(settings.get("FOURPLAY_WEBRTC_ICE_UDP_PORT_START", DEFAULT_ICE_UDP_PORT_START)),
                            int(settings.get("FOURPLAY_WEBRTC_ICE_UDP_PORT_COUNT", DEFAULT_ICE_UDP_PORT_COUNT))).validate_service_pools(settings)
    control = int(settings.get("FOURPLAY_CONTROL_PLANE_BIND", "0.0.0.0:8080").rsplit(":", 1)[-1])
    if not 1 <= control <= 65535:
        raise ValueError("control-plane TCP port must be in 1-65535")
    if any("%" in source for source in sources):
        raise ValueError("scoped source addresses are not supported; use a literal host/subnet")
    networks = list(dict.fromkeys(ipaddress.ip_network(source, strict=False) for source in sources))
    if any(network.prefixlen == 0 for network in networks):
        raise ValueError("specify trusted hosts/subnets, not an unrestricted /0 source")
    lines = ["4-Play WebRTC firewall plan - PRINT ONLY; no rules applied",
             f"Control-plane signaling: TCP {control}", f"Browser ICE/media/input: UDP {ports.start}-{ports.end}",
             "ICE-TCP fallback is disabled. Publisher HTTP ports remain loopback-only.",
             "Review sources and existing rules before applying any command.", "", "UFW commands:"]
    for source in networks:
        lines += [f"sudo ufw allow from {source} to any port {control} proto tcp comment '4-Play web signaling'",
                  f"sudo ufw allow from {source} to any port {ports.start}:{ports.end} proto udp comment '4-Play WebRTC ICE'"]
    lines += ["", "nftables examples (use your actual table/chain; do not mix firewall managers):"]
    for source in networks:
        family = "ip" if source.version == 4 else "ip6"
        lines += [f"sudo nft add rule inet filter input {family} saddr {source} tcp dport {control} accept",
                  f"sudo nft add rule inet filter input {family} saddr {source} udp dport {ports.start}-{ports.end} accept"]
    lines += ["", "This does not alter router ACLs, Windows firewall, NAT, or the UDP/FFplay path."]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--env-file", help="Read selected literal values from this systemd env file; no shell execution")
    parser.add_argument("--source", action="append", required=True, help="Trusted IPv4/IPv6 host or subnet; repeat as needed")
    args = parser.parse_args()
    try:
        print(plan(read_settings(args.env_file, os.environ), args.source))
    except (ValueError, OSError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
