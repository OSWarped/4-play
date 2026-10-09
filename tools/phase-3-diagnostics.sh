#!/usr/bin/env bash
set -u

control_plane_url="${FOURPLAY_CONTROL_PLANE_URL:-http://127.0.0.1:8080}"
seat_api_token="${FOURPLAY_SEAT_API_TOKEN:-phase-1c-seat-token-2026}"
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

section() {
    printf '\n== %s ==\n' "$1"
}

run() {
    printf '$ %s\n' "$*"
    "$@" || true
}

curl_json() {
    local path="$1"
    printf '$ curl %s%s\n' "$control_plane_url" "$path"
    curl -fsS \
        -H "Authorization: Bearer ${seat_api_token}" \
        "${control_plane_url}${path}" || true
    printf '\n'
}

section "4-Play Phase 3 diagnostics"
printf 'repository=%s\n' "$repository_root"
printf 'control_plane_url=%s\n' "$control_plane_url"

section "systemd services"
run systemctl --no-pager --full status 4play-control-plane.service
run systemctl --no-pager --full status 4play-runtime-host-agent.service

section "control plane"
run curl -fsS "${control_plane_url}/health"
printf '\n'
run curl -fsS "${control_plane_url}/ready"
printf '\n'
curl_json "/api/v1/games"
curl_json "/api/v1/sessions"
curl_json "/api/v1/runtime-hosts"

section "catalog metadata"
if [[ -x "$repository_root/target/release/catalog-admin" ]]; then
    FOURPLAY_CONTROL_PLANE_URL="$control_plane_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
        "$repository_root/target/release/catalog-admin" report || true
    FOURPLAY_CONTROL_PLANE_URL="$control_plane_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
        "$repository_root/target/release/catalog-admin" validate-assets || true
else
    printf 'catalog-admin release binary not found at %s\n' "$repository_root/target/release/catalog-admin"
fi

section "listeners"
run ss -ltnp
run ss -lunp

section "runtime processes"
run ps -ef

section "uinput"
run ls -l /dev/uinput
run id

section "4-Play virtual input devices"
if compgen -G "/sys/class/input/event*/device/name" > /dev/null; then
    grep -H "4-Play Session" /sys/class/input/event*/device/name || true
else
    printf 'no input event devices found\n'
fi

section "recent host-agent logs"
if [[ -d /tmp/4play/host-agent ]]; then
    find /tmp/4play/host-agent -maxdepth 1 -type f -name '*.log' -printf '%T@ %p\n' \
        | sort -nr \
        | head -5 \
        | cut -d' ' -f2- \
        | while read -r log_file; do
            printf '\n-- %s --\n' "$log_file"
            tail -40 "$log_file" || true
        done
else
    printf '/tmp/4play/host-agent does not exist\n'
fi

