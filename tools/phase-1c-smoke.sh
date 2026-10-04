#!/usr/bin/env bash

set -uo pipefail

results_directory="${1:-/tmp/4play-phase-1c-smoke-$(date +%Y%m%d-%H%M%S)}"
script_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_directory/.." && pwd)"
control_plane_binary="$repository_root/target/release/control-plane-server"
agent_binary="$repository_root/target/release/runtime-host-agent"
session_runtime_binary="$repository_root/target/release/session-runtime"
bind_address="127.0.0.1:41810"
base_url="http://$bind_address"
host_id="phase-1c-smoke"
runtime_state_directory="$results_directory/runtime"

control_plane_pid=""
agent_pid=""
failures=0
last_status=0

mkdir -p "$results_directory" "$runtime_state_directory"
summary_path="$results_directory/summary.txt"
: >"$summary_path"

pass() {
    printf 'PASS\t%s\n' "$1" | tee -a "$summary_path"
}

fail() {
    printf 'FAIL\t%s\n' "$1" | tee -a "$summary_path" >&2
    failures=$((failures + 1))
}

process_running() {
    local state
    state="$(ps -p "$1" -o stat= 2>/dev/null | tr -d ' ' || true)"
    [[ -n "$state" && "$state" != Z* ]]
}

terminate_process() {
    local pid="$1"
    last_status=0
    if [[ -z "$pid" ]]; then
        return
    fi
    if process_running "$pid"; then
        kill -TERM "$pid" 2>/dev/null || true
    fi
    for ((attempt = 0; attempt < 100; attempt++)); do
        if ! process_running "$pid"; then
            break
        fi
        sleep 0.1
    done
    if process_running "$pid"; then
        kill -KILL "$pid" 2>/dev/null || true
    fi
    wait "$pid" 2>/dev/null || last_status=$?
}

cleanup_runtime_processes() {
    local pid_file pid session_id command_line
    shopt -s nullglob
    for pid_file in "$runtime_state_directory"/*.pid; do
        pid="$(tr -d '[:space:]' <"$pid_file")"
        session_id="$(basename "$pid_file" .pid)"
        if [[ "$pid" =~ ^[0-9]+$ ]] && process_running "$pid"; then
            command_line="$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null || true)"
            if [[ "$command_line" == *"--session-id $session_id"* ]]; then
                terminate_process "$pid"
            fi
        fi
    done
    shopt -u nullglob
}

cleanup() {
    terminate_process "$agent_pid"
    cleanup_runtime_processes
    terminate_process "$control_plane_pid"
}

trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

wait_for_url() {
    local url="$1"
    for ((attempt = 0; attempt < 150; attempt++)); do
        if curl -fsS "$url" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

json_field() {
    python3 -c 'import json,sys; value=json.load(open(sys.argv[1])); print(value[sys.argv[2]])' "$1" "$2"
}

wait_for_session_state() {
    local session_id="$1"
    local expected="$2"
    local output_path="$3"
    local state
    for ((attempt = 0; attempt < 300; attempt++)); do
        if curl -fsS "$base_url/api/v1/sessions/$session_id" >"$output_path" 2>/dev/null; then
            state="$(json_field "$output_path" state 2>/dev/null || true)"
            if [[ "$state" == "$expected" ]]; then
                return 0
            fi
            if [[ "$state" =~ ^(allocation_failed|launch_failed|runtime_lost|terminated)$ ]] \
                && [[ "$state" != "$expected" ]]; then
                return 1
            fi
        fi
        sleep 0.1
    done
    return 1
}

create_session() {
    local seat_id="$1"
    local output_path="$2"
    curl -fsS -X POST \
        -H 'content-type: application/json' \
        --data "{\"game_id\":\"tmnt\",\"seat_id\":\"$seat_id\",\"destination_address\":\"127.0.0.1\"}" \
        "$base_url/api/v1/sessions" >"$output_path"
}

for binary in "$control_plane_binary" "$agent_binary" "$session_runtime_binary"; do
    if [[ -x "$binary" ]]; then
        pass "release binary exists: $(basename "$binary")"
    else
        fail "release binary exists: $binary"
    fi
done
for command_name in curl grep pgrep ps python3 ss; do
    if command -v "$command_name" >/dev/null 2>&1; then
        pass "required command is installed: $command_name"
    else
        fail "required command is installed: $command_name"
    fi
done
if [[ -r /dev/uinput && -w /dev/uinput ]]; then
    pass "runtime user can access /dev/uinput"
else
    fail "runtime user can access /dev/uinput"
fi
if ss -H -ltn "sport = :41810" | grep -q .; then
    fail "TCP port 41810 is available"
else
    pass "TCP port 41810 is available"
fi
for port in 41000 42000; do
    if ss -H -lun "sport = :$port" | grep -q .; then
        fail "UDP port $port is available"
    else
        pass "UDP port $port is available"
    fi
done
if ((failures > 0)); then
    exit 2
fi

control_plane_log="$results_directory/control-plane.log"
agent_log="$results_directory/agent.log"
FOURPLAY_CONTROL_PLANE_BIND="$bind_address" \
FOURPLAY_CONTROL_PLANE_DATABASE="$results_directory/control-plane.sqlite3" \
FOURPLAY_RUNTIME_HOST_OFFLINE_SECONDS=3 \
    "$control_plane_binary" >"$control_plane_log" 2>&1 &
control_plane_pid=$!

if wait_for_url "$base_url/ready"; then
    pass "isolated control plane becomes ready"
else
    fail "isolated control plane becomes ready"
fi

FOURPLAY_CONTROL_PLANE_URL="$base_url" \
FOURPLAY_RUNTIME_HOST_ID="$host_id" \
FOURPLAY_RUNTIME_HOST_NAME="Phase 1C Smoke Host" \
FOURPLAY_RUNTIME_HOST_ADDRESS="127.0.0.1" \
FOURPLAY_HEARTBEAT_SECONDS=1 \
FOURPLAY_RECONCILE_MILLISECONDS=100 \
FOURPLAY_SESSION_RUNTIME_PATH="$session_runtime_binary" \
FOURPLAY_RUNTIME_STATE_DIRECTORY="$runtime_state_directory" \
    "$agent_binary" >"$agent_log" 2>&1 &
agent_pid=$!

if wait_for_url "$base_url/api/v1/games/tmnt"; then
    pass "seat can browse the verified TMNT catalog entry"
else
    fail "seat can browse the verified TMNT catalog entry"
fi

first_json="$results_directory/session-normal-create.json"
if create_session "phase-1c-normal" "$first_json"; then
    pass "seat requests and allocates a normal session"
else
    fail "seat requests and allocates a normal session"
fi
first_session_id="$(json_field "$first_json" id 2>/dev/null || true)"
first_state_json="$results_directory/session-normal-state.json"
if [[ -n "$first_session_id" ]] \
    && wait_for_session_state "$first_session_id" active "$first_state_json"; then
    pass "agent launches MAME and reports the session active"
else
    fail "agent launches MAME and reports the session active"
fi

first_pid_file="$runtime_state_directory/$first_session_id.pid"
first_pid="$(tr -d '[:space:]' <"$first_pid_file" 2>/dev/null || true)"
if [[ "$first_pid" =~ ^[0-9]+$ ]] && process_running "$first_pid"; then
    pass "active session runtime process is supervised"
else
    fail "active session runtime process is supervised"
fi

normal_stop_json="$results_directory/session-normal-stop.json"
if curl -fsS -X POST "$base_url/api/v1/sessions/$first_session_id/stop" \
    >"$normal_stop_json" \
    && wait_for_session_state "$first_session_id" stopped "$first_state_json"; then
    pass "normal stop reaches the durable stopped state"
else
    fail "normal stop reaches the durable stopped state"
fi
if [[ -z "$first_pid" ]] || ! process_running "$first_pid"; then
    pass "normal stop reaps the session runtime"
else
    fail "normal stop reaps the session runtime"
fi

failure_create_json="$results_directory/session-failure-create.json"
if create_session "phase-1c-failure" "$failure_create_json"; then
    pass "seat can request another session without rebooting"
else
    fail "seat can request another session without rebooting"
fi
failure_session_id="$(json_field "$failure_create_json" id 2>/dev/null || true)"
failure_state_json="$results_directory/session-failure-state.json"
if [[ -n "$failure_session_id" ]] \
    && wait_for_session_state "$failure_session_id" active "$failure_state_json"; then
    pass "replacement session becomes active"
else
    fail "replacement session becomes active"
fi

failure_pid="$(tr -d '[:space:]' <"$runtime_state_directory/$failure_session_id.pid" 2>/dev/null || true)"
mame_pid=""
if [[ "$failure_pid" =~ ^[0-9]+$ ]]; then
    for ((attempt = 0; attempt < 100; attempt++)); do
        mame_pid="$(pgrep -P "$failure_pid" -x mame | head -n 1 || true)"
        [[ -n "$mame_pid" ]] && break
        sleep 0.1
    done
fi
if [[ -n "$mame_pid" ]]; then
    kill -KILL "$mame_pid"
    pass "test injects an unexpected MAME process failure"
else
    fail "test injects an unexpected MAME process failure"
fi
if wait_for_session_state "$failure_session_id" runtime_lost "$failure_state_json"; then
    pass "unexpected MAME exit reaches diagnosable runtime_lost state"
else
    fail "unexpected MAME exit reaches diagnosable runtime_lost state"
fi
if grep -Fq 'MAME exited unexpectedly' "$runtime_state_directory/$failure_session_id.log"; then
    pass "runtime failure reason is retained in the session log"
else
    fail "runtime failure reason is retained in the session log"
fi

games_after_failure="$results_directory/games-after-failure.json"
if curl -fsS "$base_url/api/v1/games" >"$games_after_failure" \
    && grep -Fq '"id":"tmnt"' "$games_after_failure" \
    && process_running "$agent_pid"; then
    pass "seat returns to browsing while the runtime host remains healthy"
else
    fail "seat returns to browsing while the runtime host remains healthy"
fi

sessions_json="$results_directory/sessions.json"
if curl -fsS "$base_url/api/v1/sessions" >"$sessions_json" \
    && grep -Fq '"state":"stopped"' "$sessions_json" \
    && grep -Fq '"state":"runtime_lost"' "$sessions_json"; then
    pass "control plane retains normal and failure lifecycle evidence"
else
    fail "control plane retains normal and failure lifecycle evidence"
fi

terminate_process "$agent_pid"
agent_status=$last_status
agent_pid=""
if [[ "$agent_status" -eq 0 ]]; then
    pass "runtime-host agent exits cleanly"
else
    fail "runtime-host agent exits cleanly (status $agent_status)"
fi
cleanup_runtime_processes
terminate_process "$control_plane_pid"
control_plane_status=$last_status
control_plane_pid=""
if [[ "$control_plane_status" -eq 0 ]]; then
    pass "control plane exits cleanly"
else
    fail "control plane exits cleanly (status $control_plane_status)"
fi

printf '\nResults: %s\n' "$results_directory" | tee -a "$summary_path"
if ((failures == 0)); then
    printf 'OVERALL\tPASS\n' | tee -a "$summary_path"
else
    printf 'OVERALL\tFAIL (%d assertions)\n' "$failures" | tee -a "$summary_path" >&2
fi

trap - EXIT INT TERM
cleanup
exit "$((failures > 0))"
