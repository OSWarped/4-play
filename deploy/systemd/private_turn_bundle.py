"""Render the reviewed IPv4 TURN sandbox candidate; never install or activate it."""
import ipaddress

VERSION = "4.18.0"


def addresses(local, public):
    local, public = ipaddress.ip_address(local), ipaddress.ip_address(public)
    if local.version != 4 or not local.is_private or local.is_loopback or local.is_unspecified or local.is_link_local:
        raise ValueError("A private, non-loopback IPv4 runtime host address is required")
    if public.version != 4 or not public.is_global:
        raise ValueError("A globally routable public IPv4 address is required")
    return str(local), str(public)


def egress_rules(uid, local, public):
    local, public = addresses(local, public)
    if type(uid) is not int or not 1 <= uid < 2**31:
        raise ValueError("A non-root dedicated service UID is required")
    # This table never flushes or alters UFW/global rules. A second installation
    # must explicitly review/remove this exact table while TURN is stopped.
    return f"""table inet fourplay_turn {{
  chain output {{
    type filter hook output priority -10; policy accept;
    meta skuid != {uid} return
    meta nfproto ipv6 counter drop
    udp sport 3478 ct state established counter accept
    tcp sport 3478 ct state established counter accept
    ip daddr {{ {local}, {public} }} udp dport 50000-50999 counter accept
    ip daddr {public} udp dport 52000-52199 counter accept
    ip daddr {{ {local}, {public} }} counter drop
    ip daddr {{ 0.0.0.0/8, 10.0.0.0/8, 100.64.0.0/10, 127.0.0.0/8, 169.254.0.0/16, 172.16.0.0/12, 192.0.0.0/24, 192.0.2.0/24, 192.168.0.0/16, 198.18.0.0/15, 198.51.100.0/24, 203.0.113.0/24, 224.0.0.0/3 }} counter drop
    udp sport 52000-52199 counter accept
    counter drop
  }}
}}
"""


def turn_config(local, public, secret):
    import re
    local, public = addresses(local, public)
    if not re.fullmatch(r"[a-f0-9]{64}", secret):
        raise ValueError("TURN requires a random 64-hex shared secret")
    # Never log this return value. It is intended for a root-only credential
    # loaded into the service's private /run/credentials directory by systemd.
    return f"""listening-port=3478
listening-ip={local}
relay-ip={local}
external-ip={public}/{local}
min-port=52000
max-port=52199
realm=4play-private-test
fingerprint
use-auth-secret
static-auth-secret={secret}
no-tls
no-dtls
no-cli
no-tcp-relay
no-multicast-peers
no-rfc5780
no-software-attribute
denied-peer-ip=0.0.0.0-0.255.255.255
denied-peer-ip=10.0.0.0-10.255.255.255
denied-peer-ip=100.64.0.0-100.127.255.255
denied-peer-ip=127.0.0.0-127.255.255.255
denied-peer-ip=169.254.0.0-169.254.255.255
denied-peer-ip=172.16.0.0-172.31.255.255
denied-peer-ip=192.168.0.0-192.168.255.255
denied-peer-ip=224.0.0.0-255.255.255.255
allowed-peer-ip={local}
user-quota=4
total-quota=32
max-bps=3000000
bps-capacity=24000000
relay-threads=1
pidfile=/run/4play-turn/turnserver.pid
log-file=stdout
simple-log
"""


def service_units():
    return {"4play-turn.service": f"""[Unit]
Description=4-Play private authenticated TURN relay
Requires=4play-turn-egress.service
After=network-online.target 4play-turn-egress.service
Wants=network-online.target

[Service]
Type=simple
User=turnserver
Group=turnserver
LoadCredential=turnserver.conf:/etc/4play-turn/turnserver.conf
ExecStart=/opt/4play-coturn/{VERSION}/turnserver -c %d/turnserver.conf
RuntimeDirectory=4play-turn
RuntimeDirectoryMode=0700
UMask=0077
Restart=on-failure
RestartSec=3
NoNewPrivileges=yes
CapabilityBoundingSet=
AmbientCapabilities=
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes
RestrictRealtime=yes
LockPersonality=yes
RestrictAddressFamilies=AF_UNIX AF_INET
MemoryDenyWriteExecute=yes
MemoryMax=256M
TasksMax=64
LimitNOFILE=4096
TimeoutStopSec=15

[Install]
WantedBy=multi-user.target
""", "4play-turn-egress.service": """[Unit]
Description=4-Play dedicated TURN UID outbound restrictions
Before=4play-turn.service

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/sbin/nft -f /etc/4play-turn/egress.nft
# Intentionally retain restrictions on stop. Removing/replacing this exact
# table requires TURN to be stopped first; never flush the global ruleset.
"""}
