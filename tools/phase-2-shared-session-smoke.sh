#!/usr/bin/env bash

set -uo pipefail

results_directory="${1:-/tmp/4play-phase-2-shared-session-smoke-$(date +%Y%m%d-%H%M%S)}"
script_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_directory/.." && pwd)"
control_plane_binary="$repository_root/target/release/control-plane-server"
agent_binary="$repository_root/target/release/runtime-host-agent"
session_runtime_binary="$repository_root/target/release/session-runtime"
catalog_admin_binary="$repository_root/target/release/catalog-admin"
seat_input_binary="$repository_root/target/release/seat-input"
bind_address="${FOURPLAY_PHASE2_SMOKE_BIND:-127.0.0.1:41820}"
base_url="http://$bind_address"
host_id="phase-2-smoke"
runtime_state_directory="$results_directory/runtime"
asset_root="$results_directory/assets"
seat_api_token="phase2-seat-$(date +%s%N)-$$"
runtime_host_api_token="phase2-host-$(date +%s%N)-$$"
media_port_start="${FOURPLAY_PHASE2_SMOKE_MEDIA_PORT_START:-43100}"
input_port_start="${FOURPLAY_PHASE2_SMOKE_INPUT_PORT_START:-44100}"
preview_stale_ms=600

control_plane_pid=""
agent_pid=""
session_id=""
spectator_grant_id=""
failures=0
last_status=0

mkdir -p "$results_directory" "$runtime_state_directory" "$asset_root/previews"
summary_path="$results_directory/summary.txt"
: >"$summary_path"

pass() {
    printf 'PASS\t%s\n' "$1" | tee -a "$summary_path"
}

fail() {
    printf 'FAIL\t%s\n' "$1" | tee -a "$summary_path" >&2
    failures=$((failures + 1))
}

curl() {
    command curl -H "authorization: Bearer $seat_api_token" "$@"
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
    local pid_file pid runtime_session_id command_line
    shopt -s nullglob
    for pid_file in "$runtime_state_directory"/*.pid; do
        pid="$(tr -d '[:space:]' <"$pid_file")"
        runtime_session_id="$(basename "$pid_file" .pid)"
        if [[ "$pid" =~ ^[0-9]+$ ]] && process_running "$pid"; then
            command_line="$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null || true)"
            if [[ "$command_line" == *"--session-id $runtime_session_id"* ]]; then
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

json_expr() {
    python3 -c 'import json,sys; print(eval(sys.argv[2], {"data": json.load(open(sys.argv[1]))}))' "$1" "$2"
}

wait_for_session_state() {
    local expected="$1"
    local output_path="$2"
    local state
    for ((attempt = 0; attempt < 400; attempt++)); do
        if curl -fsS "$base_url/api/v1/sessions/$session_id" >"$output_path" 2>/dev/null; then
            state="$(json_expr "$output_path" 'data["state"]' 2>/dev/null || true)"
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

wait_for_summary_status() {
    local expected="$1"
    local output_path="$2"
    local status
    for ((attempt = 0; attempt < 200; attempt++)); do
        if curl -fsS "$base_url/api/v1/active-sessions" >"$output_path" 2>/dev/null; then
            status="$(json_expr "$output_path" 'data["sessions"][0]["preview_status"] if data["sessions"] else ""' 2>/dev/null || true)"
            if [[ "$status" == "$expected" ]]; then
                return 0
            fi
        fi
        sleep 0.1
    done
    return 1
}

post_json() {
    local path="$1"
    local body="$2"
    local output_path="$3"
    curl -fsS -X POST -H 'content-type: application/json' --data "$body" "$base_url$path" >"$output_path"
}

delete_json() {
    local path="$1"
    local body="$2"
    local output_path="$3"
    curl -fsS -X DELETE -H 'content-type: application/json' --data "$body" "$base_url$path" >"$output_path"
}

for binary in "$control_plane_binary" "$agent_binary" "$session_runtime_binary" "$catalog_admin_binary" "$seat_input_binary"; do
    if [[ -x "$binary" ]]; then
        pass "release binary exists: $(basename "$binary")"
    else
        fail "release binary exists: $binary"
    fi
done
for command_name in curl grep python3 ps ss; do
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
tcp_port="${bind_address##*:}"
if ss -H -ltn "sport = :$tcp_port" | grep -q .; then
    fail "TCP port $tcp_port is available"
else
    pass "TCP port $tcp_port is available"
fi
for port in "$media_port_start" "$input_port_start"; do
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
FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
FOURPLAY_RUNTIME_HOST_API_TOKEN="$runtime_host_api_token" \
FOURPLAY_MEDIA_PORT_START="$media_port_start" \
FOURPLAY_MEDIA_PORT_COUNT=100 \
FOURPLAY_INPUT_PORT_START="$input_port_start" \
FOURPLAY_INPUT_PORT_COUNT=100 \
FOURPLAY_ASSET_ROOT="$asset_root" \
FOURPLAY_PREVIEW_STALE_MS="$preview_stale_ms" \
    "$control_plane_binary" >"$control_plane_log" 2>&1 &
control_plane_pid=$!

if wait_for_url "$base_url/ready"; then
    pass "isolated Phase 2 control plane becomes ready"
else
    fail "isolated Phase 2 control plane becomes ready"
fi

FOURPLAY_CONTROL_PLANE_URL="$base_url" \
FOURPLAY_RUNTIME_HOST_ID="$host_id" \
FOURPLAY_RUNTIME_HOST_NAME="Phase 2 Smoke Host" \
FOURPLAY_RUNTIME_HOST_ADDRESS="127.0.0.1" \
FOURPLAY_HEARTBEAT_SECONDS=1 \
FOURPLAY_RECONCILE_MILLISECONDS=2000 \
FOURPLAY_RUNTIME_HOST_API_TOKEN="$runtime_host_api_token" \
FOURPLAY_SESSION_RUNTIME_PATH="$session_runtime_binary" \
FOURPLAY_RUNTIME_STATE_DIRECTORY="$runtime_state_directory" \
FOURPLAY_ASSET_ROOT="$asset_root" \
FOURPLAY_PREVIEW_INTERVAL_MS=200 \
FOURPLAY_MAME_PATH="$HOME/src/mame-4play/mame" \
    "$agent_binary" >"$agent_log" 2>&1 &
agent_pid=$!

if wait_for_url "$base_url/api/v1/games/tmnt"; then
    pass "seat can browse the TMNT catalog entry"
else
    fail "seat can browse the TMNT catalog entry"
fi

admin_report_before="$results_directory/catalog-report-before.txt"
if FOURPLAY_CONTROL_PLANE_URL="$base_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
    "$catalog_admin_binary" report >"$admin_report_before"; then
    pass "catalog-admin can report isolated library metadata completeness"
else
    fail "catalog-admin can report isolated library metadata completeness"
fi

if FOURPLAY_CONTROL_PLANE_URL="$base_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
    "$catalog_admin_binary" seed-known-metadata >"$results_directory/seed-known-metadata.txt"; then
    pass "catalog-admin seeds known catalog presentation metadata"
else
    fail "catalog-admin seeds known catalog presentation metadata"
fi

if FOURPLAY_CONTROL_PLANE_URL="$base_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
    "$catalog_admin_binary" seed-placeholders --asset-root "$asset_root" --update-metadata \
    >"$results_directory/seed-placeholders.txt"; then
    pass "catalog-admin seeds placeholder assets in the isolated asset root"
else
    fail "catalog-admin seeds placeholder assets in the isolated asset root"
fi

if FOURPLAY_CONTROL_PLANE_URL="$base_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
    "$catalog_admin_binary" validate-assets >"$results_directory/validate-assets.txt"; then
    pass "catalog-admin validates placeholder assets through the asset endpoint"
else
    fail "catalog-admin validates placeholder assets through the asset endpoint"
fi

admin_report_after="$results_directory/catalog-report-after.txt"
if FOURPLAY_CONTROL_PLANE_URL="$base_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
    "$catalog_admin_binary" report >"$admin_report_after" \
    && grep -Fq 'complete: 4' "$admin_report_after" \
    && grep -Fq 'incomplete: 0' "$admin_report_after"; then
    pass "catalog-admin reports complete browser metadata after seeding"
else
    fail "catalog-admin reports complete browser metadata after seeding"
fi

seat_browser_output="$results_directory/seat-browser-list.txt"
if "$seat_input_binary" \
    --control-plane "$base_url" \
    --api-token "$seat_api_token" \
    --seat-id phase-2-browser-seat \
    --destination-ip 127.0.0.1 \
    --list-only >"$seat_browser_output" \
    && grep -Fq 'Available games:' "$seat_browser_output" \
    && grep -Fq 'about: Four-player arcade beat' "$seat_browser_output" \
    && grep -Fq 'controls: Move with the stick; jump and attack can be pressed together for special moves.' "$seat_browser_output" \
    && grep -Fq 'media: artwork, marquee, screenshot, logo' "$seat_browser_output" \
    && grep -Fq 'primary image: media/tmnt/screenshot.svg' "$seat_browser_output"; then
    pass "seat browser renders seeded catalog metadata without launching a game"
else
    fail "seat browser renders seeded catalog metadata without launching a game"
fi

create_json="$results_directory/create-session.json"
if post_json "/api/v1/sessions" \
    '{"game_id":"tmnt","seat_id":"phase-2-seat-1","destination_address":"127.0.0.1"}' \
    "$create_json"; then
    pass "seat 1 starts a shared TMNT session"
else
    fail "seat 1 starts a shared TMNT session"
fi
session_id="$(json_expr "$create_json" 'data["id"]' 2>/dev/null || true)"
state_json="$results_directory/session-state.json"
if [[ -n "$session_id" ]] && wait_for_session_state active "$state_json"; then
    pass "runtime host launches the shared session"
else
    fail "runtime host launches the shared session"
fi

summary_json="$results_directory/summary-still.json"
if wait_for_summary_status still_available "$summary_json" \
    && grep -Fq "\"previews/$session_id.bmp\"" "$summary_json" \
    && grep -Fq '"preview_updated_unix_ms":' "$summary_json"; then
    pass "active-session discovery exposes a fresh still preview"
else
    fail "active-session discovery exposes a fresh still preview"
fi
preview_file="$results_directory/preview.bmp"
if curl -fsS "$base_url/api/v1/assets/previews/$session_id.bmp" >"$preview_file" \
    && [[ "$(dd if="$preview_file" bs=2 count=1 2>/dev/null)" == "BM" ]]; then
    pass "published still preview is available through the asset endpoint"
else
    fail "published still preview is available through the asset endpoint"
fi
sleep 0.8
summary_stale_json="$results_directory/summary-stale.json"
if wait_for_summary_status stale_available "$summary_stale_json"; then
    pass "active-session discovery marks old still previews stale"
else
    fail "active-session discovery marks old still previews stale"
fi

reserve_json="$results_directory/reserve-p2.json"
if post_json "/api/v1/sessions/$session_id/player-slots/2/reserve" \
    '{"seat_id":"phase-2-seat-2"}' "$reserve_json" \
    && grep -Fq '"state":"reserved"' "$reserve_json" \
    && grep -Fq '"seat_id":"phase-2-seat-2"' "$reserve_json"; then
    pass "seat 2 atomically reserves player 2"
else
    fail "seat 2 atomically reserves player 2"
fi

connect_json="$results_directory/connect-p2.json"
if post_json "/api/v1/sessions/$session_id/player-slots/2/connect" \
    '{"seat_id":"phase-2-seat-2"}' "$connect_json" \
    && grep -Fq '"state":"occupied"' "$connect_json"; then
    pass "seat 2 connects to player 2"
else
    fail "seat 2 connects to player 2"
fi

disconnect_json="$results_directory/disconnect-p2.json"
if post_json "/api/v1/sessions/$session_id/player-slots/2/disconnect" \
    '{"seat_id":"phase-2-seat-2"}' "$disconnect_json" \
    && grep -Fq '"state":"disconnected"' "$disconnect_json"; then
    pass "seat 2 disconnects into a reconnectable lease"
else
    fail "seat 2 disconnects into a reconnectable lease"
fi

reconnect_json="$results_directory/reconnect-p2.json"
if post_json "/api/v1/sessions/$session_id/player-slots/2/connect" \
    '{"seat_id":"phase-2-seat-2"}' "$reconnect_json" \
    && grep -Fq '"state":"occupied"' "$reconnect_json"; then
    pass "same seat reconnects to player 2"
else
    fail "same seat reconnects to player 2"
fi

release_json="$results_directory/release-p2.json"
if post_json "/api/v1/sessions/$session_id/player-slots/2/release" \
    '{"seat_id":"phase-2-seat-2"}' "$release_json" \
    && grep -Fq '"state":"open"' "$release_json"; then
    pass "seat 2 releases player 2 back to open"
else
    fail "seat 2 releases player 2 back to open"
fi

spectator_json="$results_directory/spectator-grant.json"
if post_json "/api/v1/sessions/$session_id/spectators" \
    '{"seat_id":"phase-2-spectator","destination_address":"127.0.0.1"}' \
    "$spectator_json"; then
    pass "spectator receives a distinct media grant"
else
    fail "spectator receives a distinct media grant"
fi
spectator_grant_id="$(json_expr "$spectator_json" 'data["id"]' 2>/dev/null || true)"
spectator_port="$(json_expr "$spectator_json" 'data["media_udp_port"]' 2>/dev/null || true)"
if [[ "$spectator_port" =~ ^[0-9]+$ ]] && [[ "$spectator_port" != "$media_port_start" ]]; then
    pass "spectator media port is distinct from the primary session port"
else
    fail "spectator media port is distinct from the primary session port"
fi
summary_spectator_json="$results_directory/summary-spectator.json"
if curl -fsS "$base_url/api/v1/active-sessions" >"$summary_spectator_json" \
    && grep -Fq '"active_spectator_count":1' "$summary_spectator_json"; then
    pass "active-session discovery reports spectator count"
else
    fail "active-session discovery reports spectator count"
fi
assignments_json="$results_directory/runtime-assignments.json"
if curl -fsS "$base_url/api/v1/runtime-hosts/$host_id/sessions" >"$assignments_json" \
    && grep -Fq "\"spectator_media_ports\":[$spectator_port]" "$assignments_json"; then
    pass "runtime assignments expose spectator media ports"
else
    fail "runtime assignments expose spectator media ports"
fi

release_spectator_json="$results_directory/release-spectator.json"
if [[ -n "$spectator_grant_id" ]] \
    && delete_json "/api/v1/sessions/$session_id/spectators/$spectator_grant_id" \
        '{"seat_id":"phase-2-spectator"}' "$release_spectator_json" \
    && grep -Fq '"active_spectator_count":0' "$release_spectator_json"; then
    pass "spectator grant releases without affecting player slots"
else
    fail "spectator grant releases without affecting player slots"
fi

stop_json="$results_directory/stop-session.json"
if curl -fsS -X POST "$base_url/api/v1/sessions/$session_id/stop" >"$stop_json" \
    && wait_for_session_state stopped "$state_json"; then
    pass "shared session stops cleanly"
else
    fail "shared session stops cleanly"
fi
session_pid=""
if [[ -r "$runtime_state_directory/$session_id.pid" ]]; then
    session_pid="$(tr -d '[:space:]' <"$runtime_state_directory/$session_id.pid" 2>/dev/null || true)"
fi
if [[ -z "$session_pid" ]] || ! process_running "$session_pid"; then
    pass "shared session runtime process is reaped"
else
    fail "shared session runtime process is reaped"
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
