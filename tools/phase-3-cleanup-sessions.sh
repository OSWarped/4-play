#!/usr/bin/env bash
set -euo pipefail

control_plane_url="${FOURPLAY_CONTROL_PLANE_URL:-http://127.0.0.1:8080}"
seat_api_token="${FOURPLAY_SEAT_API_TOKEN:-phase-1c-seat-token-2026}"
timeout_seconds="${FOURPLAY_PHASE3_CLEANUP_TIMEOUT_SECONDS:-45}"
poll_interval_seconds="${FOURPLAY_PHASE3_CLEANUP_POLL_SECONDS:-1}"
run_smoke="${FOURPLAY_PHASE3_CLEANUP_SMOKE:-0}"

terminal_states_regex='^(stopped|allocation_failed|launch_failed|runtime_lost|terminated)$'

usage() {
    cat <<EOF
Usage: $(basename "$0") [--smoke]

Environment:
  FOURPLAY_CONTROL_PLANE_URL                  Default: http://127.0.0.1:8080
  FOURPLAY_SEAT_API_TOKEN                     Default: phase-1c-seat-token-2026
  FOURPLAY_PHASE3_CLEANUP_TIMEOUT_SECONDS     Default: 45
  FOURPLAY_PHASE3_CLEANUP_POLL_SECONDS        Default: 1
  FOURPLAY_PHASE3_CLEANUP_SMOKE               Set to 1 to run strict-idle smoke after cleanup
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --smoke)
            run_smoke=1
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            printf 'unknown option: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_directory="${FOURPLAY_PHASE3_CLEANUP_RESULTS:-/tmp/4play-phase-3-cleanup-$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$work_directory"

sessions_json="$work_directory/sessions-before.json"
remaining_json="$work_directory/sessions-after.json"

curl_auth() {
    curl -fsS -H "Authorization: Bearer ${seat_api_token}" "$@"
}

curl_auth "${control_plane_url}/api/v1/sessions" >"$sessions_json"

mapfile -t active_sessions < <(python3 - "$sessions_json" "$terminal_states_regex" <<'PY'
import json
import re
import sys

path = sys.argv[1]
terminal = re.compile(sys.argv[2])
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
for session in payload.get("sessions", []):
    state = session.get("state", "")
    if not terminal.match(state):
        print(f"{session.get('id')} {session.get('game_id')} {state}")
PY
)

if [[ "${#active_sessions[@]}" -eq 0 ]]; then
    printf 'No non-terminal sessions found.\n'
else
    printf 'Requesting stop for %s non-terminal session(s):\n' "${#active_sessions[@]}"
    for entry in "${active_sessions[@]}"; do
        read -r session_id game_id state <<<"$entry"
        printf '  %s %s %s\n' "$session_id" "$game_id" "$state"
        curl_auth -X POST "${control_plane_url}/api/v1/sessions/${session_id}/stop" \
            >"$work_directory/stop-${session_id}.json" || {
                printf '  warning: failed to request stop for %s\n' "$session_id" >&2
            }
    done
fi

deadline=$((SECONDS + timeout_seconds))
while true; do
    curl_auth "${control_plane_url}/api/v1/sessions" >"$remaining_json"
    remaining_count="$(python3 - "$remaining_json" "$terminal_states_regex" <<'PY'
import json
import re
import sys

path = sys.argv[1]
terminal = re.compile(sys.argv[2])
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
remaining = [
    session for session in payload.get("sessions", [])
    if not terminal.match(session.get("state", ""))
]
for session in remaining:
    print(f"{session.get('id')} {session.get('game_id')} {session.get('state')}", file=sys.stderr)
print(len(remaining))
PY
)"
    if [[ "$remaining_count" == "0" ]]; then
        printf 'All sessions are terminal.\n'
        break
    fi
    if [[ "$SECONDS" -ge "$deadline" ]]; then
        printf 'Timed out waiting for sessions to become terminal. Remaining: %s\n' "$remaining_count" >&2
        printf 'Artifacts: %s\n' "$work_directory" >&2
        exit 1
    fi
    sleep "$poll_interval_seconds"
done

printf 'Cleanup artifacts: %s\n' "$work_directory"

if [[ "$run_smoke" == "1" ]]; then
    printf '\nRunning strict-idle Phase 3 smoke...\n'
    FOURPLAY_CONTROL_PLANE_URL="$control_plane_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
    FOURPLAY_PHASE3_STRICT_IDLE=1 \
        "$repository_root/tools/phase-3-systemd-smoke.sh"
fi

