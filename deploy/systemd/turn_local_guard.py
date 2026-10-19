"""Temporary host-only ingress gate for TURN commissioning; no global flush."""


def rules():
    return """table inet fourplay_turn_local_only {
  chain input {
    type filter hook input priority -20; policy accept;
    iifname "lo" return
    ct state established,related return
    udp dport { 3478, 52000-52199 } counter drop
    tcp dport 3478 counter drop
  }
}
"""
