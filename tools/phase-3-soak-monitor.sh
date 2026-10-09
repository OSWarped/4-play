#!/usr/bin/env bash
set -euo pipefail

control_plane_url="${FOURPLAY_CONTROL_PLANE_URL:-http://127.0.0.1:8080}"
seat_api_token="${FOURPLAY_SEAT_API_TOKEN:-phase-1c-seat-token-2026}"
duration_seconds="${FOURPLAY_PHASE3_SOAK_SECONDS:-3600}"
sample_seconds="${FOURPLAY_PHASE3_SOAK_SAMPLE_SECONDS:-30}"
cleanup_after="${FOURPLAY_PHASE3_SOAK_CLEANUP:-0}"
strict_smoke_after="${FOURPLAY_PHASE3_SOAK_STRICT_SMOKE:-0}"
results_directory="${FOURPLAY_PHASE3_SOAK_RESULTS:-/tmp/4play-phase-3-soak-$(date +%Y%m%d-%H%M%S)}"

usage() {
    cat <<EOF
Usage: $(basename "$0") [--seconds <duration>] [--sample-seconds <interval>] [--cleanup] [--strict-smoke]

Monitors a manual Phase 3 table soak. It does not launch seats or games.
Run it on the Linux server before or during a play session.

Environment:
  FOURPLAY_CONTROL_PLANE_URL             Default: http://127.0.0.1:8080
  FOURPLAY_SEAT_API_TOKEN                Default: phase-1c-seat-token-2026
  FOURPLAY_PHASE3_SOAK_SECONDS           Default: 3600
  FOURPLAY_PHASE3_SOAK_SAMPLE_SECONDS    Default: 30
  FOURPLAY_PHASE3_SOAK_RESULTS           Default: /tmp/4play-phase-3-soak-<timestamp>
  FOURPLAY_PHASE3_SOAK_CLEANUP           Set to 1 to stop sessions after monitoring
  FOURPLAY_PHASE3_SOAK_STRICT_SMOKE      Set to 1 to run strict smoke after monitoring
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --seconds)
            duration_seconds="${2:-}"
            if [[ -z "$duration_seconds" ]]; then
                printf '--seconds requires a value\n' >&2
                exit 2
            fi
            shift
            ;;
        --sample-seconds)
            sample_seconds="${2:-}"
            if [[ -z "$sample_seconds" ]]; then
                printf '--sample-seconds requires a value\n' >&2
                exit 2
            fi
            shift
            ;;
        --cleanup)
            cleanup_after=1
            ;;
        --strict-smoke)
            strict_smoke_after=1
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

if ! [[ "$duration_seconds" =~ ^[1-9][0-9]*$ ]]; then
    printf 'duration must be a positive integer\n' >&2
    exit 2
fi

if ! [[ "$sample_seconds" =~ ^[1-9][0-9]*$ ]]; then
    printf 'sample interval must be a positive integer\n' >&2
    exit 2
fi

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$results_directory/samples"

curl_auth() {
    curl -fsS -H "Authorization: Bearer ${seat_api_token}" "$@"
}

sample() {
    local index="$1"
    local timestamp
    timestamp="$(date --iso-8601=seconds)"
    local prefix
    prefix="$results_directory/samples/$(printf '%04d' "$index")"

    printf '%s\n' "$timestamp" >"${prefix}-timestamp.txt"
    curl -fsS "${control_plane_url}/health" >"${prefix}-health.txt" 2>"${prefix}-health.err" || true
    curl -fsS "${control_plane_url}/ready" >"${prefix}-ready.txt" 2>"${prefix}-ready.err" || true
    curl_auth "${control_plane_url}/api/v1/games" >"${prefix}-games.json" 2>"${prefix}-games.err" || true
    curl_auth "${control_plane_url}/api/v1/sessions" >"${prefix}-sessions.json" 2>"${prefix}-sessions.err" || true
    curl_auth "${control_plane_url}/api/v1/runtime-hosts" >"${prefix}-runtime-hosts.json" 2>"${prefix}-runtime-hosts.err" || true
    ps -eo pid,ppid,comm,%cpu,%mem,rss,etime,args >"${prefix}-processes.txt" || true
    ss -ltnp >"${prefix}-tcp-listeners.txt" 2>"${prefix}-tcp-listeners.err" || true
    ss -lunp >"${prefix}-udp-listeners.txt" 2>"${prefix}-udp-listeners.err" || true
}

summarize() {
    python3 - "$results_directory" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
samples = sorted((root / "samples").glob("*-sessions.json"))
print("sample\tactive_sessions\tsession_states\truntime_hosts")
for sessions_path in samples:
    sample_id = sessions_path.name.split("-", 1)[0]
    hosts_path = sessions_path.with_name(f"{sample_id}-runtime-hosts.json")
    active_count = 0
    try:
        sessions = json.loads(sessions_path.read_text(encoding="utf-8")).get("sessions", [])
    except Exception as error:
        session_states = f"sessions_error:{error}"
    else:
        active = [
            session for session in sessions
            if session.get("state") not in {"stopped", "allocation_failed", "launch_failed", "runtime_lost", "terminated"}
        ]
        active_count = len(active)
        state_counts = {}
        for session in sessions:
            state_counts[session.get("state", "unknown")] = state_counts.get(session.get("state", "unknown"), 0) + 1
        session_states = ",".join(f"{state}:{count}" for state, count in sorted(state_counts.items())) or "none"
    try:
        host_payload = json.loads(hosts_path.read_text(encoding="utf-8"))
        hosts = host_payload.get("hosts", host_payload.get("runtime_hosts", []))
    except Exception as error:
        host_summary = f"hosts_error:{error}"
    else:
        host_summary = ",".join(f"{host.get('id')}:{host.get('status')}" for host in hosts) or "none"
    print(f"{sample_id}\t{active_count}\t{session_states}\t{host_summary}")
PY
}

printf 'Phase 3 soak monitor results: %s\n' "$results_directory"
printf 'control_plane_url=%s\n' "$control_plane_url"
printf 'duration_seconds=%s sample_seconds=%s\n' "$duration_seconds" "$sample_seconds"

deadline=$((SECONDS + duration_seconds))
sample_index=0
while true; do
    sample "$sample_index"
    sample_index=$((sample_index + 1))
    if [[ "$SECONDS" -ge "$deadline" ]]; then
        break
    fi
    remaining=$((deadline - SECONDS))
    if (( remaining < sample_seconds )); then
        sleep "$remaining"
    else
        sleep "$sample_seconds"
    fi
done

summarize | tee "$results_directory/summary.tsv"

if [[ -x "$repository_root/tools/phase-3-diagnostics.sh" ]]; then
    "$repository_root/tools/phase-3-diagnostics.sh" >"$results_directory/diagnostics.txt" 2>"$results_directory/diagnostics.err" || true
fi

if [[ "$cleanup_after" == "1" ]]; then
    FOURPLAY_CONTROL_PLANE_URL="$control_plane_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
        "$repository_root/tools/phase-3-cleanup-sessions.sh" \
        >"$results_directory/cleanup.txt" 2>"$results_directory/cleanup.err"
fi

if [[ "$strict_smoke_after" == "1" ]]; then
    FOURPLAY_CONTROL_PLANE_URL="$control_plane_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
    FOURPLAY_PHASE3_STRICT_IDLE=1 \
    FOURPLAY_PHASE3_STRICT_METADATA=1 \
        "$repository_root/tools/phase-3-systemd-smoke.sh" \
        >"$results_directory/strict-smoke.txt" 2>"$results_directory/strict-smoke.err"
    tail -n 5 "$results_directory/strict-smoke.txt"
fi

printf 'Soak artifacts: %s\n' "$results_directory"
