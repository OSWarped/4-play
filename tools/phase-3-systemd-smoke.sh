#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
control_plane_url="${FOURPLAY_CONTROL_PLANE_URL:-http://127.0.0.1:8080}"
seat_api_token="${FOURPLAY_SEAT_API_TOKEN:-phase-1c-seat-token-2026}"
expected_host_id="${FOURPLAY_RUNTIME_HOST_ID:-reference-linux}"
minimum_game_count="${FOURPLAY_PHASE3_MINIMUM_GAME_COUNT:-14}"
strict_idle="${FOURPLAY_PHASE3_STRICT_IDLE:-0}"
strict_metadata="${FOURPLAY_PHASE3_STRICT_METADATA:-0}"

pass_count=0
fail_count=0
warn_count=0
results_directory="${FOURPLAY_PHASE3_SMOKE_RESULTS:-/tmp/4play-phase-3-systemd-smoke-$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$results_directory"

pass() {
    pass_count=$((pass_count + 1))
    printf '[PASS] %s\n' "$1"
}

fail() {
    fail_count=$((fail_count + 1))
    printf '[FAIL] %s\n' "$1"
}

warn() {
    warn_count=$((warn_count + 1))
    printf '[WARN] %s\n' "$1"
}

capture() {
    local name="$1"
    shift
    "$@" >"$results_directory/${name}.out" 2>"$results_directory/${name}.err"
}

assert_command() {
    local description="$1"
    shift
    if capture "$description" "$@"; then
        pass "$description"
    else
        fail "$description"
    fi
}

printf 'Phase 3 systemd smoke results: %s\n' "$results_directory"

assert_command "control-plane-service-active" \
    systemctl is-active --quiet 4play-control-plane.service

assert_command "runtime-host-agent-service-active" \
    systemctl is-active --quiet 4play-runtime-host-agent.service

assert_command "control-plane-health" \
    curl -fsS "${control_plane_url}/health"

assert_command "control-plane-ready" \
    curl -fsS "${control_plane_url}/ready"

games_json="$results_directory/games.json"
sessions_json="$results_directory/sessions.json"
hosts_json="$results_directory/runtime-hosts.json"
report_json="$results_directory/catalog-report.json"
assets_json="$results_directory/asset-validation.json"

if curl -fsS -H "Authorization: Bearer ${seat_api_token}" \
    "${control_plane_url}/api/v1/games" >"$games_json"; then
    pass "catalog endpoint responds"
else
    fail "catalog endpoint responds"
fi

if curl -fsS -H "Authorization: Bearer ${seat_api_token}" \
    "${control_plane_url}/api/v1/sessions" >"$sessions_json"; then
    pass "sessions endpoint responds"
else
    fail "sessions endpoint responds"
fi

if curl -fsS -H "Authorization: Bearer ${seat_api_token}" \
    "${control_plane_url}/api/v1/runtime-hosts" >"$hosts_json"; then
    pass "runtime-hosts endpoint responds"
else
    fail "runtime-hosts endpoint responds"
fi

if python3 - "$games_json" "$minimum_game_count" <<'PY'
import json
import sys

path = sys.argv[1]
minimum = int(sys.argv[2])
with open(path, encoding="utf-8") as handle:
    data = json.load(handle)
games = data.get("games", [])
if len(games) < minimum:
    raise SystemExit(f"expected at least {minimum} games, found {len(games)}")
missing = [game.get("id", "<unknown>") for game in games if not game.get("availability")]
if missing:
    raise SystemExit(f"games missing availability: {', '.join(missing)}")
print(f"games={len(games)}")
PY
then
    pass "catalog publishes curated games with availability"
else
    fail "catalog publishes curated games with availability"
fi

if python3 - "$hosts_json" "$expected_host_id" <<'PY'
import json
import sys

path = sys.argv[1]
expected = sys.argv[2]
with open(path, encoding="utf-8") as handle:
    data = json.load(handle)
hosts = data.get("hosts", [])
host = next((item for item in hosts if item.get("id") == expected), None)
if host is None:
    raise SystemExit(f"missing host {expected}")
if host.get("status") != "online":
    raise SystemExit(f"host {expected} is {host.get('status')}")
print(f"host={expected} status=online")
PY
then
    pass "runtime host is online"
else
    fail "runtime host is online"
fi

if python3 - "$sessions_json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
sessions = data.get("sessions", [])
active = [
    session for session in sessions
    if session.get("state") not in {"Stopped", "Failed"}
]
if active:
    raise SystemExit(f"expected idle server, found {len(active)} active sessions")
print("active_sessions=0")
PY
then
    pass "server starts idle"
else
    if [[ "$strict_idle" == "1" ]]; then
        fail "server starts idle"
    else
        warn "server is not idle; set FOURPLAY_PHASE3_STRICT_IDLE=1 to fail this check"
    fi
fi

if [[ -x "$repository_root/target/release/catalog-admin" ]]; then
    if FOURPLAY_CONTROL_PLANE_URL="$control_plane_url" \
        FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
        "$repository_root/target/release/catalog-admin" report --json >"$report_json"; then
        pass "catalog-admin report succeeds"
    else
        fail "catalog-admin report succeeds"
    fi

    if python3 - "$report_json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)
if report.get("incomplete_count") != 0:
    raise SystemExit(f"incomplete_count={report.get('incomplete_count')}")
print(f"complete={report.get('complete_count')}")
PY
    then
        pass "catalog metadata is complete"
    else
        if [[ "$strict_metadata" == "1" ]]; then
            fail "catalog metadata is complete"
        else
            warn "catalog metadata is incomplete; set FOURPLAY_PHASE3_STRICT_METADATA=1 to fail this check"
        fi
    fi

    if FOURPLAY_CONTROL_PLANE_URL="$control_plane_url" \
        FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
        "$repository_root/target/release/catalog-admin" validate-assets --json >"$assets_json"; then
        pass "catalog-admin asset validation succeeds"
    else
        fail "catalog-admin asset validation succeeds"
    fi

    if python3 - "$assets_json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)
if report.get("missing_count") != 0:
    raise SystemExit(f"missing_count={report.get('missing_count')}")
print(f"checked={report.get('checked_count')}")
PY
    then
        pass "catalog assets are present"
    else
        fail "catalog assets are present"
    fi
else
    fail "catalog-admin release binary exists"
fi

if [[ -e /dev/uinput ]] && [[ -r /dev/uinput ]] && [[ -w /dev/uinput ]]; then
    pass "/dev/uinput is accessible"
else
    fail "/dev/uinput is accessible"
fi

printf '\nPhase 3 systemd smoke summary: %s passed, %s warned, %s failed\n' \
    "$pass_count" "$warn_count" "$fail_count"
printf 'Artifacts: %s\n' "$results_directory"

if [[ "$fail_count" -ne 0 ]]; then
    exit 1
fi
