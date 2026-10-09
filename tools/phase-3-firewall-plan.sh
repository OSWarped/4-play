#!/usr/bin/env bash
set -euo pipefail

env_file="${FOURPLAY_ENV_FILE:-/etc/4play/4play.env}"
trusted_source="${FOURPLAY_TRUSTED_SOURCE:-192.168.20.0/24}"

if [[ -f "$env_file" ]]; then
    # shellcheck disable=SC1090
    set -a
    source "$env_file"
    set +a
fi

control_bind="${FOURPLAY_CONTROL_PLANE_BIND:-0.0.0.0:8080}"
media_start="${FOURPLAY_MEDIA_PORT_START:-41000}"
media_count="${FOURPLAY_MEDIA_PORT_COUNT:-100}"
input_start="${FOURPLAY_INPUT_PORT_START:-42000}"
input_count="${FOURPLAY_INPUT_PORT_COUNT:-100}"

port_from_bind() {
    local bind="$1"
    printf '%s\n' "${bind##*:}"
}

range_end() {
    local start="$1"
    local count="$2"
    if ! [[ "$start" =~ ^[0-9]+$ && "$count" =~ ^[0-9]+$ ]]; then
        printf 'port start and count must be numeric\n' >&2
        return 1
    fi
    if (( count < 1 )); then
        printf 'port count must be greater than zero\n' >&2
        return 1
    fi
    local end=$((start + count - 1))
    if (( start < 1 || end > 65535 )); then
        printf 'port range %s+%s is outside 1-65535\n' "$start" "$count" >&2
        return 1
    fi
    printf '%s\n' "$end"
}

control_port="$(port_from_bind "$control_bind")"
media_end="$(range_end "$media_start" "$media_count")"
input_end="$(range_end "$input_start" "$input_count")"

cat <<EOF
4-Play Phase 3 firewall plan
  env file: ${env_file}
  trusted source: ${trusted_source}
  control plane: tcp/${control_port}
  media UDP range: ${media_start}:${media_end}
  input UDP range: ${input_start}:${input_end}

ufw commands:
sudo ufw allow from ${trusted_source} to any port ${control_port} proto tcp comment '4-play control plane'
sudo ufw allow from ${trusted_source} to any port ${media_start}:${media_end} proto udp comment '4-play media range'
sudo ufw allow from ${trusted_source} to any port ${input_start}:${input_end} proto udp comment '4-play seat input range'

nftables equivalent:
nft add rule inet filter input ip saddr ${trusted_source} tcp dport ${control_port} accept
nft add rule inet filter input ip saddr ${trusted_source} udp dport ${media_start}-${media_end} accept
nft add rule inet filter input ip saddr ${trusted_source} udp dport ${input_start}-${input_end} accept
EOF
