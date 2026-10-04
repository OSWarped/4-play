#!/usr/bin/env bash

set -uo pipefail

results_directory="${1:-/tmp/4play-runtime-host-smoke-$(date +%Y%m%d-%H%M%S)}"
script_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_directory/.." && pwd)"
control_plane_binary="$repository_root/target/debug/control-plane-server"
agent_binary="$repository_root/target/debug/runtime-host-agent"
bind_address="127.0.0.1:41800"
base_url="http://$bind_address"
host_id="reference-linux-smoke"

control_plane_pid=""
agent_pid=""
failures=0
last_status=0

mkdir -p "$results_directory"
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
    for ((attempt = 0; attempt < 50; attempt++)); do
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

cleanup() {
    terminate_process "$agent_pid"
    terminate_process "$control_plane_pid"
}

trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

wait_for_url() {
    local url="$1"
    for ((attempt = 0; attempt < 100; attempt++)); do
        if curl -fsS "$url" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

wait_for_log() {
    local path="$1"
    local pattern="$2"
    for ((attempt = 0; attempt < 100; attempt++)); do
        if [[ -f "$path" ]] && grep -Eq "$pattern" "$path"; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

if [[ ! -x "$control_plane_binary" ]]; then
    fail "control-plane binary exists: $control_plane_binary"
fi
if [[ ! -x "$agent_binary" ]]; then
    fail "runtime-host agent binary exists: $agent_binary"
fi
for command_name in curl grep ps ss; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        fail "required command is installed: $command_name"
    fi
done
if ss -H -ltn "sport = :41800" | grep -q .; then
    fail "TCP port 41800 is available"
else
    pass "TCP port 41800 is available"
fi
if ((failures > 0)); then
    exit 2
fi

control_plane_log="$results_directory/control-plane.log"
agent_log="$results_directory/agent.log"
host_json="$results_directory/host.json"
hosts_json="$results_directory/hosts.json"

FOURPLAY_CONTROL_PLANE_BIND="$bind_address" \
    "$control_plane_binary" >"$control_plane_log" 2>&1 &
control_plane_pid=$!

if wait_for_url "$base_url/health"; then
    pass "control plane becomes healthy"
else
    fail "control plane becomes healthy"
fi

FOURPLAY_CONTROL_PLANE_URL="$base_url" \
FOURPLAY_RUNTIME_HOST_ID="$host_id" \
FOURPLAY_RUNTIME_HOST_NAME="Reference Linux Smoke Host" \
FOURPLAY_HEARTBEAT_SECONDS=1 \
    "$agent_binary" >"$agent_log" 2>&1 &
agent_pid=$!

if wait_for_log "$agent_log" 'Runtime host registered: id=reference-linux-smoke'; then
    pass "agent registers the reference host"
else
    fail "agent registers the reference host"
fi
if wait_for_log "$agent_log" 'Heartbeat accepted: host=reference-linux-smoke sequence=2'; then
    pass "agent sends recurring sequenced heartbeats"
else
    fail "agent sends recurring sequenced heartbeats"
fi

if curl -fsS "$base_url/api/v1/runtime-hosts/$host_id" >"$host_json"; then
    pass "registered host is available through lookup"
else
    fail "registered host is available through lookup"
fi
if curl -fsS "$base_url/api/v1/runtime-hosts" >"$hosts_json"; then
    pass "registered host is available through listing"
else
    fail "registered host is available through listing"
fi

if grep -Fq '"operating_system":"linux"' "$host_json"; then
    pass "agent reports Linux operating system"
else
    fail "agent reports Linux operating system"
fi
if grep -Eq '"logical_cpu_count":[1-9][0-9]*' "$host_json"; then
    pass "agent reports a nonzero logical CPU count"
else
    fail "agent reports a nonzero logical CPU count"
fi
if grep -Eq '"memory_bytes":[1-9][0-9]*' "$host_json"; then
    pass "agent reports nonzero system memory"
else
    fail "agent reports nonzero system memory"
fi
if grep -Eq '"encoder_names":\["' "$host_json"; then
    pass "agent discovers at least one supported H.264 encoder"
else
    fail "agent discovers at least one supported H.264 encoder"
fi
if grep -Fq '"emulator_adapters":["mame"]' "$host_json"; then
    pass "agent discovers the MAME adapter"
else
    fail "agent discovers the MAME adapter"
fi
if grep -Fq '"id":"reference-linux-smoke"' "$hosts_json"; then
    pass "host listing contains the registered identity"
else
    fail "host listing contains the registered identity"
fi

terminate_process "$agent_pid"
agent_status=$last_status
agent_pid=""
if [[ "$agent_status" -eq 0 ]]; then
    pass "agent exits cleanly after SIGTERM"
else
    fail "agent exits cleanly after SIGTERM (status $agent_status)"
fi

terminate_process "$control_plane_pid"
control_plane_status=$last_status
control_plane_pid=""
if [[ "$control_plane_status" -eq 0 ]]; then
    pass "control plane exits cleanly after SIGTERM"
else
    fail "control plane exits cleanly after SIGTERM (status $control_plane_status)"
fi

if ss -H -ltn "sport = :41800" | grep -q .; then
    fail "control plane releases TCP port 41800"
else
    pass "control plane releases TCP port 41800"
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
