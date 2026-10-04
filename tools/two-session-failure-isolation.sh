#!/usr/bin/env bash

set -uo pipefail

post_failure_seconds="${1:-5}"
results_directory="${2:-/tmp/4play-failure-isolation-$(date +%Y%m%d-%H%M%S)}"

if ! [[ "$post_failure_seconds" =~ ^[1-9][0-9]*$ ]]; then
    echo "post-failure duration must be a positive integer" >&2
    exit 2
fi

script_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_directory/.." && pwd)"
runtime_binary="$repository_root/target/debug/session-runtime"
injector_binary="$repository_root/target/debug/input-inject"

session_a_id="$(date +%s)"
session_b_id=$((session_a_id + 1))
stream_a_port=41401
stream_b_port=41402
input_a_port=42401
input_b_port=42402

runtime_a_pid=""
runtime_b_pid=""
receiver_a_pid=""
receiver_b_pid=""
mame_a_pid=""
mame_b_pid=""
encoder_a_pid=""
encoder_b_pid=""
event_a=""
event_b=""
runtime_a_log=""
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
    local attempts="${2:-100}"

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

cleanup() {
    terminate_process "$runtime_a_pid" 30
    terminate_process "$runtime_b_pid" 30
    terminate_process "$receiver_a_pid" 20
    terminate_process "$receiver_b_pid" 20

    for pid in "$mame_a_pid" "$mame_b_pid" "$encoder_a_pid" "$encoder_b_pid"; do
        if [[ -n "$pid" ]] && process_running "$pid"; then
            kill -KILL "$pid" 2>/dev/null || true
        fi
    done
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

wait_for_process_exit() {
    local pid="$1"
    local timeout_seconds="$2"

    for ((attempt = 0; attempt < timeout_seconds * 10; attempt++)); do
        if ! process_running "$pid"; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

wait_for_port_free() {
    local port="$1"
    local timeout_seconds="$2"

    for ((attempt = 0; attempt < timeout_seconds * 10; attempt++)); do
        if ! ss -H -lun "sport = :$port" | grep -q .; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

controller_event_for_session() {
    local session_id="$1"
    local expected_name="4-Play Session $session_id Player 1"
    local name_path name event_name

    shopt -s nullglob
    for name_path in /sys/class/input/event*/device/name; do
        IFS= read -r name <"$name_path" || continue
        if [[ "$name" == "$expected_name" ]]; then
            event_name="$(basename "$(dirname "$(dirname "$name_path")")")"
            printf '/dev/input/%s\n' "$event_name"
            shopt -u nullglob
            return 0
        fi
    done
    shopt -u nullglob
    return 1
}

wait_for_controller_event() {
    local session_id="$1"
    local timeout_seconds="$2"

    discovered_event=""
    for ((attempt = 0; attempt < timeout_seconds * 10; attempt++)); do
        discovered_event="$(controller_event_for_session "$session_id" || true)"
        if [[ -n "$discovered_event" ]]; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

last_video_frames() {
    grep -Eo 'video_frames=[0-9]+' "$1" 2>/dev/null | tail -n 1 | cut -d= -f2
}

assert_process_stopped() {
    local pid="$1"
    local description="$2"
    if [[ -n "$pid" ]] && ! process_running "$pid"; then
        pass "$description"
    else
        fail "$description"
    fi
}

assert_transient_resources_removed() {
    local session_id="$1"
    local input_port="$2"
    local event_path="$3"
    local label="$4"
    local working_directory="/tmp/4play/session-$session_id"

    if [[ -n "$event_path" && ! -e "$event_path" ]]; then
        pass "$label removed its virtual controller"
    else
        fail "$label removed its virtual controller"
    fi

    if wait_for_port_free "$input_port" 5; then
        pass "$label released UDP input port $input_port"
    else
        fail "$label released UDP input port $input_port"
    fi

    for endpoint in video.raw audio.pcm; do
        if [[ ! -e "$working_directory/$endpoint" ]]; then
            pass "$label removed transient endpoint $endpoint"
        else
            fail "$label removed transient endpoint $endpoint"
        fi
    done

    if [[ -d "$working_directory/nvram" && -d "$working_directory/cfg" ]]; then
        pass "$label preserved its persistent NVRAM and configuration directories"
    else
        fail "$label preserved its persistent NVRAM and configuration directories"
    fi
}

exercise_controller() {
    local label="$1"
    local event_path="$2"
    local destination="$3"
    local expected_event="$4"
    shift 4
    local monitor_pid injector_status
    local event_log="$results_directory/${label}-event.log"

    timeout 2s stdbuf -oL -eL evtest "$event_path" >"$event_log" 2>&1 &
    monitor_pid=$!
    sleep 0.25
    "$injector_binary" "$destination" "$@" \
        >"$results_directory/${label}-inject.log" 2>&1
    injector_status=$?
    wait "$monitor_pid" 2>/dev/null || true

    if [[ "$injector_status" -eq 0 ]] && grep -Fq "$expected_event" "$event_log"; then
        pass "$label delivered controller input"
    else
        fail "$label delivered controller input"
    fi
}

assert_session_b_survives() {
    local label="$1"
    local frames_before="$2"
    local frames_after

    sleep "$post_failure_seconds"
    frames_after="$(last_video_frames "$results_directory/runtime-b.log")"

    if [[ -n "$frames_before" && -n "$frames_after" ]] && ((frames_after > frames_before)); then
        pass "session B video advances after $label ($frames_before -> $frames_after)"
    else
        fail "session B video advances after $label"
    fi

    if process_running "$runtime_b_pid" \
        && process_running "$mame_b_pid" \
        && process_running "$encoder_b_pid"; then
        pass "session B process tree survives $label"
    else
        fail "session B process tree survives $label"
    fi

    if [[ -n "$event_b" && -e "$event_b" ]]; then
        pass "session B controller survives $label"
    else
        fail "session B controller survives $label"
    fi
}

start_session_a() {
    local phase="$1"
    runtime_a_log="$results_directory/runtime-a-$phase.log"

    setsid "$runtime_binary" \
        --session-id "$session_a_id" \
        --rom tmnt \
        --width 320 \
        --height 224 \
        --fps 60.000000 \
        --destination-ip 127.0.0.1 \
        --udp-port "$stream_a_port" \
        --input-port "$input_a_port" \
        --audio-codec aac \
        --audio-block-ms 20 \
        --audio-thread-queue-size 4 \
        >"$runtime_a_log" 2>&1 &
    runtime_a_pid=$!

    if wait_for_log "$runtime_a_log" "Waiting for seat controller state" 20; then
        pass "session A reached its input loop during $phase"
    else
        fail "session A reached its input loop during $phase"
    fi

    if wait_for_controller_event "$session_a_id" 10; then
        event_a="$discovered_event"
        pass "session A created its controller during $phase: $event_a"
    else
        event_a=""
        fail "session A created its controller during $phase"
    fi

    mame_a_pid="$(pgrep -P "$runtime_a_pid" -x mame | head -n 1 || true)"
    encoder_a_pid="$(pgrep -P "$runtime_a_pid" -x ffmpeg | head -n 1 || true)"

    if [[ -n "$mame_a_pid" ]] && process_running "$mame_a_pid"; then
        pass "session A owns MAME during $phase (PID $mame_a_pid)"
    else
        fail "session A owns MAME during $phase"
    fi
    if [[ -n "$encoder_a_pid" ]] && process_running "$encoder_a_pid"; then
        pass "session A owns FFmpeg during $phase (PID $encoder_a_pid)"
    else
        fail "session A owns FFmpeg during $phase"
    fi

    if wait_for_log "$runtime_a_log" "video_frames=[1-9]" 20; then
        pass "session A produces video during $phase"
    else
        fail "session A produces video during $phase"
    fi
}

reap_failed_session_a() {
    local label="$1"
    local expected_pattern="$2"
    local runtime_status=0

    if wait_for_process_exit "$runtime_a_pid" 15; then
        pass "session A runtime exits after $label"
    else
        fail "session A runtime exits after $label"
        terminate_process "$runtime_a_pid" 30
    fi

    wait "$runtime_a_pid" 2>/dev/null || runtime_status=$?
    runtime_a_pid=""

    if [[ "$runtime_status" -ne 0 ]]; then
        pass "session A reports $label as a failure (status $runtime_status)"
    else
        fail "session A reports $label as a failure"
    fi

    if grep -Fq "$expected_pattern" "$runtime_a_log"; then
        pass "session A logs the $label cause"
    else
        fail "session A logs the $label cause"
    fi
}

for command_name in evtest ffmpeg grep pgrep ps setsid ss stdbuf timeout; do
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
    if ss -H -lun "sport = :$port" | grep -q .; then
        fail "UDP port $port is available"
    else
        pass "UDP port $port is available"
    fi
done
if ((failures > 0)); then
    exit 2
fi

ffmpeg -nostdin -hide_banner -loglevel info \
    -fflags nobuffer -flags low_delay -probesize 32768 -analyzeduration 1000000 \
    -i "udp://127.0.0.1:$stream_a_port?fifo_size=1000000&overrun_nonfatal=1" \
    -map 0:v:0 -map 0:a:0 -f null - \
    >"$results_directory/receiver-a.log" 2>&1 &
receiver_a_pid=$!

ffmpeg -nostdin -hide_banner -loglevel info \
    -fflags nobuffer -flags low_delay -probesize 32768 -analyzeduration 1000000 \
    -i "udp://127.0.0.1:$stream_b_port?fifo_size=1000000&overrun_nonfatal=1" \
    -map 0:v:0 -map 0:a:0 -f null - \
    >"$results_directory/receiver-b.log" 2>&1 &
receiver_b_pid=$!
sleep 0.25

start_session_a initial

setsid "$runtime_binary" \
    --session-id "$session_b_id" \
    --rom aliens \
    --width 288 \
    --height 224 \
    --fps 59.185606 \
    --destination-ip 127.0.0.1 \
    --udp-port "$stream_b_port" \
    --input-port "$input_b_port" \
    --audio-codec aac \
    --audio-block-ms 20 \
    --audio-thread-queue-size 4 \
    >"$results_directory/runtime-b.log" 2>&1 &
runtime_b_pid=$!

if wait_for_log "$results_directory/runtime-b.log" "Waiting for seat controller state" 20; then
    pass "session B reached its input loop"
else
    fail "session B reached its input loop"
fi
if wait_for_controller_event "$session_b_id" 10; then
    event_b="$discovered_event"
    pass "session B created its controller: $event_b"
else
    fail "session B created its controller"
fi

mame_b_pid="$(pgrep -P "$runtime_b_pid" -x mame | head -n 1 || true)"
encoder_b_pid="$(pgrep -P "$runtime_b_pid" -x ffmpeg | head -n 1 || true)"

if wait_for_log "$results_directory/runtime-b.log" "video_frames=[1-9]" 20; then
    pass "session B produces video"
else
    fail "session B produces video"
fi
if wait_for_log "$results_directory/receiver-a.log" "320x224" 20; then
    pass "receiver A decoded the initial TMNT stream"
else
    fail "receiver A decoded the initial TMNT stream"
fi
if wait_for_log "$results_directory/receiver-b.log" "288x224" 20; then
    pass "receiver B decoded the Aliens stream"
else
    fail "receiver B decoded the Aliens stream"
fi

exercise_controller \
    initial-b "$event_b" "127.0.0.1:$input_b_port" "BTN_SOUTH), value 1" \
    --buttons action1 --axis-y -1 --hold-ms 300

frames_before="$(last_video_frames "$results_directory/runtime-b.log")"
kill -KILL "$mame_a_pid"
reap_failed_session_a "MAME SIGKILL" "MAME exited unexpectedly"
assert_process_stopped "$mame_a_pid" "session A MAME is gone after MAME SIGKILL"
assert_process_stopped "$encoder_a_pid" "session A FFmpeg is reaped after MAME SIGKILL"
assert_transient_resources_removed "$session_a_id" "$input_a_port" "$event_a" \
    "session A after MAME SIGKILL"
assert_session_b_survives "session A MAME SIGKILL" "$frames_before"
exercise_controller \
    after-mame-failure-b "$event_b" "127.0.0.1:$input_b_port" "BTN_EAST), value 1" \
    --buttons action2 --axis-x 1 --hold-ms 300

start_session_a after-mame-failure
exercise_controller \
    restarted-after-mame-a "$event_a" "127.0.0.1:$input_a_port" "BTN_NORTH), value 1" \
    --buttons action3 --axis-x -1 --hold-ms 300

frames_before="$(last_video_frames "$results_directory/runtime-b.log")"
kill -KILL "$encoder_a_pid"
reap_failed_session_a "FFmpeg SIGKILL" "FFmpeg exited unexpectedly"
assert_process_stopped "$encoder_a_pid" "session A FFmpeg is gone after FFmpeg SIGKILL"
assert_process_stopped "$mame_a_pid" "session A MAME is reaped after FFmpeg SIGKILL"
assert_transient_resources_removed "$session_a_id" "$input_a_port" "$event_a" \
    "session A after FFmpeg SIGKILL"
assert_session_b_survives "session A FFmpeg SIGKILL" "$frames_before"
exercise_controller \
    after-encoder-failure-b "$event_b" "127.0.0.1:$input_b_port" "BTN_WEST), value 1" \
    --buttons action4 --axis-y 1 --hold-ms 300

start_session_a after-encoder-failure
exercise_controller \
    restarted-after-encoder-a "$event_a" "127.0.0.1:$input_a_port" "BTN_TL), value 1" \
    --buttons action5 --axis-y -1 --hold-ms 300

terminate_process "$runtime_a_pid" 100
runtime_a_status=$last_status
runtime_a_pid=""
if [[ "$runtime_a_status" -eq 0 ]]; then
    pass "restarted session A exits cleanly after SIGTERM"
else
    fail "restarted session A exits cleanly after SIGTERM (status $runtime_a_status)"
fi
assert_process_stopped "$mame_a_pid" "restarted session A reaps MAME on clean shutdown"
assert_process_stopped "$encoder_a_pid" "restarted session A reaps FFmpeg on clean shutdown"
assert_transient_resources_removed "$session_a_id" "$input_a_port" "$event_a" \
    "restarted session A after clean shutdown"

terminate_process "$runtime_b_pid" 100
runtime_b_status=$last_status
runtime_b_pid=""
if [[ "$runtime_b_status" -eq 0 ]]; then
    pass "session B exits cleanly after SIGTERM"
else
    fail "session B exits cleanly after SIGTERM (status $runtime_b_status)"
fi
assert_process_stopped "$mame_b_pid" "session B reaps MAME on clean shutdown"
assert_process_stopped "$encoder_b_pid" "session B reaps FFmpeg on clean shutdown"
assert_transient_resources_removed "$session_b_id" "$input_b_port" "$event_b" \
    "session B after clean shutdown"

terminate_process "$receiver_a_pid" 20
receiver_a_pid=""
terminate_process "$receiver_b_pid" 20
receiver_b_pid=""

printf '\nResults: %s\n' "$results_directory" | tee -a "$summary_path"
if ((failures == 0)); then
    printf 'OVERALL\tPASS\n' | tee -a "$summary_path"
else
    printf 'OVERALL\tFAIL (%d assertions)\n' "$failures" | tee -a "$summary_path" >&2
fi

trap - EXIT INT TERM
cleanup
exit "$((failures > 0))"
