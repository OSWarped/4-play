#!/usr/bin/env bash

set -uo pipefail

results_directory="${1:-/tmp/4play-persistence-isolation-$(date +%Y%m%d-%H%M%S)}"
script_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_directory/.." && pwd)"
runtime_binary="$repository_root/target/debug/session-runtime"
injector_binary="$repository_root/target/debug/input-inject"

session_a_id="$(date +%s)"
session_b_id=$((session_a_id + 1))
stream_a_port=41601
stream_b_port=41602
input_a_port=42601
input_b_port=42602

runtime_a_pid=""
runtime_b_pid=""
wrapper_a_pid=""
wrapper_b_pid=""
failures=0
last_status=0

mkdir -p "$results_directory"
summary_path="$results_directory/summary.txt"
: >"$summary_path"

working_a="/tmp/4play/session-$session_a_id"
working_b="/tmp/4play/session-$session_b_id"
state_a="$working_a/state/wwfmania/auto.sta"
state_b="$working_b/state/wwfmania/auto.sta"
nvram_a="$working_a/nvram/wwfmania/nvram"
nvram_b="$working_b/nvram/wwfmania/nvram"

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
    local attempts="${2:-150}"

    last_status=0
    if [[ -z "$pid" ]]; then
        return
    fi

    if process_running "$pid"; then
        kill -TERM "$pid" 2>/dev/null || true
    fi

    for ((attempt = 0; attempt < attempts; attempt++)); do
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

terminate_traced_process() {
    local runtime_pid="$1"
    local wrapper_pid="$2"
    local attempts="${3:-150}"

    last_status=0
    if [[ -n "$runtime_pid" ]] && process_running "$runtime_pid"; then
        kill -TERM "$runtime_pid" 2>/dev/null || true
    fi

    for ((attempt = 0; attempt < attempts; attempt++)); do
        if [[ -z "$runtime_pid" ]] || ! process_running "$runtime_pid"; then
            break
        fi
        sleep 0.1
    done

    if [[ -n "$runtime_pid" ]] && process_running "$runtime_pid"; then
        kill -KILL "$runtime_pid" 2>/dev/null || true
    fi

    for ((attempt = 0; attempt < attempts; attempt++)); do
        if [[ -z "$wrapper_pid" ]] || ! process_running "$wrapper_pid"; then
            break
        fi
        sleep 0.1
    done

    if [[ -n "$wrapper_pid" ]] && process_running "$wrapper_pid"; then
        kill -TERM "$wrapper_pid" 2>/dev/null || true
        sleep 0.2
    fi
    if [[ -n "$wrapper_pid" ]] && process_running "$wrapper_pid"; then
        kill -KILL "$wrapper_pid" 2>/dev/null || true
    fi

    if [[ -n "$wrapper_pid" ]]; then
        wait "$wrapper_pid" 2>/dev/null || last_status=$?
    fi
}

cleanup() {
    if [[ -n "$wrapper_a_pid" ]]; then
        terminate_traced_process "$runtime_a_pid" "$wrapper_a_pid" 30
    else
        terminate_process "$runtime_a_pid" 30
    fi
    if [[ -n "$wrapper_b_pid" ]]; then
        terminate_traced_process "$runtime_b_pid" "$wrapper_b_pid" 30
    else
        terminate_process "$runtime_b_pid" 30
    fi
}

trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

wait_for_log() {
    local path="$1"
    local pattern="$2"
    local timeout_seconds="$3"

    for ((attempt = 0; attempt < timeout_seconds * 10; attempt++)); do
        if [[ -f "$path" ]] && grep -Eq "$pattern" "$path"; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

wait_for_child_runtime() {
    local wrapper_pid="$1"
    local timeout_seconds="$2"

    discovered_pid=""
    for ((attempt = 0; attempt < timeout_seconds * 10; attempt++)); do
        discovered_pid="$(pgrep -P "$wrapper_pid" -x session-runtime | head -n 1 || true)"
        if [[ -n "$discovered_pid" ]]; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

port_is_free() {
    local port="$1"
    ! ss -H -lun "sport = :$port" | grep -q .
}

file_hash() {
    sha256sum "$1" | cut -d' ' -f1
}

file_identity() {
    stat -c '%d:%i' "$1"
}

assert_file() {
    local path="$1"
    local description="$2"
    if [[ -s "$path" ]]; then
        pass "$description ($(stat -c %s "$path") bytes)"
    else
        fail "$description"
    fi
}

assert_trace_contains() {
    local prefix="$1"
    local pattern="$2"
    local description="$3"
    if grep -Fq "$pattern" "${prefix}"* 2>/dev/null; then
        pass "$description"
    else
        fail "$description"
    fi
}

assert_trace_excludes() {
    local prefix="$1"
    local pattern="$2"
    local description="$3"
    if grep -Fq "$pattern" "${prefix}"* 2>/dev/null; then
        fail "$description"
    else
        pass "$description"
    fi
}

inject_repeatedly() {
    local destination="$1"
    local button="$2"
    local repetitions="$3"
    local label="$4"

    for ((press = 1; press <= repetitions; press++)); do
        "$injector_binary" "$destination" \
            --buttons "$button" --hold-ms 100 \
            >>"$results_directory/${label}-inject.log" 2>&1
        sleep 0.4
    done
}

start_initial_session() {
    local label="$1"
    local session_id="$2"
    local stream_port="$3"
    local input_port="$4"
    local log_path="$5"

    setsid "$runtime_binary" \
        --session-id "$session_id" \
        --rom wwfmania \
        --width 400 \
        --height 254 \
        --fps 54.706840 \
        --destination-ip 127.0.0.1 \
        --udp-port "$stream_port" \
        --input-port "$input_port" \
        --autosave \
        --audio-codec aac \
        --audio-block-ms 20 \
        --audio-thread-queue-size 4 \
        >"$log_path" 2>&1 &

    if [[ "$label" == "A" ]]; then
        runtime_a_pid=$!
    else
        runtime_b_pid=$!
    fi
}

start_traced_session() {
    local label="$1"
    local session_id="$2"
    local stream_port="$3"
    local input_port="$4"
    local log_path="$5"
    local trace_prefix="$6"
    local wrapper_pid runtime_pid

    setsid strace -ff -e trace=openat -o "$trace_prefix" \
        "$runtime_binary" \
        --session-id "$session_id" \
        --rom wwfmania \
        --width 400 \
        --height 254 \
        --fps 54.706840 \
        --destination-ip 127.0.0.1 \
        --udp-port "$stream_port" \
        --input-port "$input_port" \
        --autosave \
        --audio-codec aac \
        --audio-block-ms 20 \
        --audio-thread-queue-size 4 \
        >"$log_path" 2>&1 &
    wrapper_pid=$!

    if wait_for_child_runtime "$wrapper_pid" 10; then
        runtime_pid="$discovered_pid"
        pass "restore session $label started under trace (runtime PID $runtime_pid)"
    else
        runtime_pid=""
        fail "restore session $label started under trace"
    fi

    if [[ "$label" == "A" ]]; then
        wrapper_a_pid="$wrapper_pid"
        runtime_a_pid="$runtime_pid"
    else
        wrapper_b_pid="$wrapper_pid"
        runtime_b_pid="$runtime_pid"
    fi
}

for command_name in ffmpeg grep pgrep ps setsid sha256sum ss stat strace; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        fail "required command is installed: $command_name"
    fi
done
if [[ ! -x "$runtime_binary" ]]; then
    fail "runtime binary exists: $runtime_binary"
fi
if [[ ! -x "$injector_binary" ]]; then
    fail "input injector exists: $injector_binary"
fi
if ((failures > 0)); then
    exit 2
fi

for port in "$stream_a_port" "$stream_b_port" "$input_a_port" "$input_b_port"; do
    if port_is_free "$port"; then
        pass "UDP port $port is available"
    else
        fail "UDP port $port is available"
    fi
done
if ((failures > 0)); then
    exit 2
fi

initial_a_log="$results_directory/initial-a.log"
initial_b_log="$results_directory/initial-b.log"
start_initial_session A "$session_a_id" "$stream_a_port" "$input_a_port" "$initial_a_log"
start_initial_session B "$session_b_id" "$stream_b_port" "$input_b_port" "$initial_b_log"

if wait_for_log "$initial_a_log" "Waiting for seat controller state" 25; then
    pass "initial session A reached its input loop"
else
    fail "initial session A reached its input loop"
fi
if wait_for_log "$initial_b_log" "Waiting for seat controller state" 25; then
    pass "initial session B reached its input loop"
else
    fail "initial session B reached its input loop"
fi
if wait_for_log "$initial_a_log" "video_frames=[1-9]" 25; then
    pass "initial session A produces video"
else
    fail "initial session A produces video"
fi
if wait_for_log "$initial_b_log" "video_frames=[1-9]" 25; then
    pass "initial session B produces video"
else
    fail "initial session B produces video"
fi

inject_repeatedly "127.0.0.1:$input_a_port" coin 1 session-a-coins
inject_repeatedly "127.0.0.1:$input_b_port" coin 5 session-b-coins
inject_repeatedly "127.0.0.1:$input_a_port" action1 2 session-a-actions
inject_repeatedly "127.0.0.1:$input_b_port" action6 4 session-b-actions
sleep 3

terminate_process "$runtime_a_pid"
initial_a_status=$last_status
runtime_a_pid=""
if [[ "$initial_a_status" -eq 0 ]]; then
    pass "initial session A exits cleanly and flushes persistence"
else
    fail "initial session A exits cleanly and flushes persistence (status $initial_a_status)"
fi
if process_running "$runtime_b_pid"; then
    pass "initial session B survives session A persistence flush"
else
    fail "initial session B survives session A persistence flush"
fi

terminate_process "$runtime_b_pid"
initial_b_status=$last_status
runtime_b_pid=""
if [[ "$initial_b_status" -eq 0 ]]; then
    pass "initial session B exits cleanly and flushes persistence"
else
    fail "initial session B exits cleanly and flushes persistence (status $initial_b_status)"
fi

assert_file "$state_a" "session A wrote a real autosave state"
assert_file "$state_b" "session B wrote a real autosave state"
assert_file "$nvram_a" "session A wrote real MAME NVRAM"
assert_file "$nvram_b" "session B wrote real MAME NVRAM"

state_a_hash="$(file_hash "$state_a")"
state_b_hash="$(file_hash "$state_b")"
nvram_a_identity="$(file_identity "$nvram_a")"
nvram_b_identity="$(file_identity "$nvram_b")"

if [[ "$state_a_hash" != "$state_b_hash" ]]; then
    pass "concurrent sessions produced distinct autosave states ($state_a_hash != $state_b_hash)"
else
    fail "concurrent sessions produced distinct autosave states"
fi
if [[ "$nvram_a_identity" != "$nvram_b_identity" ]]; then
    pass "concurrent sessions use distinct NVRAM files ($nvram_a_identity != $nvram_b_identity)"
else
    fail "concurrent sessions use distinct NVRAM files"
fi

restore_a_log="$results_directory/restore-a.log"
restore_b_log="$results_directory/restore-b.log"
trace_a_prefix="$results_directory/trace-a"
trace_b_prefix="$results_directory/trace-b"

start_traced_session A "$session_a_id" "$stream_a_port" "$input_a_port" \
    "$restore_a_log" "$trace_a_prefix"
start_traced_session B "$session_b_id" "$stream_b_port" "$input_b_port" \
    "$restore_b_log" "$trace_b_prefix"

if wait_for_log "$restore_a_log" "Waiting for seat controller state" 25 \
    && wait_for_log "$restore_a_log" "video_frames=[1-9]" 25; then
    pass "restored session A reaches running media and input"
else
    fail "restored session A reaches running media and input"
fi
if wait_for_log "$restore_b_log" "Waiting for seat controller state" 25 \
    && wait_for_log "$restore_b_log" "video_frames=[1-9]" 25; then
    pass "restored session B reaches running media and input"
else
    fail "restored session B reaches running media and input"
fi

state_b_before_a_stop="$(file_hash "$state_b")"
nvram_b_before_a_stop="$(file_hash "$nvram_b")"

terminate_traced_process "$runtime_a_pid" "$wrapper_a_pid"
restore_a_status=$last_status
runtime_a_pid=""
wrapper_a_pid=""
if [[ "$restore_a_status" -eq 0 ]]; then
    pass "restored session A exits cleanly"
else
    fail "restored session A exits cleanly (status $restore_a_status)"
fi
if process_running "$runtime_b_pid"; then
    pass "restored session B survives session A shutdown"
else
    fail "restored session B survives session A shutdown"
fi
if [[ "$(file_hash "$state_b")" == "$state_b_before_a_stop" \
    && "$(file_hash "$nvram_b")" == "$nvram_b_before_a_stop" ]]; then
    pass "session A shutdown does not modify session B persisted files"
else
    fail "session A shutdown does not modify session B persisted files"
fi

terminate_traced_process "$runtime_b_pid" "$wrapper_b_pid"
restore_b_status=$last_status
runtime_b_pid=""
wrapper_b_pid=""
if [[ "$restore_b_status" -eq 0 ]]; then
    pass "restored session B exits cleanly"
else
    fail "restored session B exits cleanly (status $restore_b_status)"
fi

assert_trace_contains "$trace_a_prefix" "$state_a" \
    "session A restore opens only its assigned autosave path"
assert_trace_contains "$trace_b_prefix" "$state_b" \
    "session B restore opens only its assigned autosave path"
assert_trace_contains "$trace_a_prefix" "$nvram_a" \
    "session A restore opens only its assigned NVRAM path"
assert_trace_contains "$trace_b_prefix" "$nvram_b" \
    "session B restore opens only its assigned NVRAM path"
assert_trace_excludes "$trace_a_prefix" "$working_b" \
    "session A never opens session B's working directory"
assert_trace_excludes "$trace_b_prefix" "$working_a" \
    "session B never opens session A's working directory"

final_state_a_hash="$(file_hash "$state_a")"
final_state_b_hash="$(file_hash "$state_b")"
final_nvram_a_identity="$(file_identity "$nvram_a")"
final_nvram_b_identity="$(file_identity "$nvram_b")"

if [[ "$final_state_a_hash" != "$final_state_b_hash" ]]; then
    pass "restored sessions remain distinct after a second save cycle"
else
    fail "restored sessions remain distinct after a second save cycle"
fi
if [[ "$final_nvram_a_identity" != "$final_nvram_b_identity" ]]; then
    pass "restored sessions retain distinct NVRAM files after a second save cycle"
else
    fail "restored sessions retain distinct NVRAM files after a second save cycle"
fi

for endpoint in "$working_a/video.raw" "$working_a/audio.pcm" \
    "$working_b/video.raw" "$working_b/audio.pcm"; do
    if [[ ! -e "$endpoint" ]]; then
        pass "transient media endpoint was removed: $endpoint"
    else
        fail "transient media endpoint was removed: $endpoint"
    fi
done
for port in "$input_a_port" "$input_b_port"; do
    if port_is_free "$port"; then
        pass "restored session released UDP input port $port"
    else
        fail "restored session released UDP input port $port"
    fi
done

printf '\nResults: %s\n' "$results_directory" | tee -a "$summary_path"
if ((failures == 0)); then
    printf 'OVERALL\tPASS\n' | tee -a "$summary_path"
else
    printf 'OVERALL\tFAIL (%d assertions)\n' "$failures" | tee -a "$summary_path" >&2
fi

trap - EXIT INT TERM
cleanup
exit "$((failures > 0))"
