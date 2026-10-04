#!/usr/bin/env bash

set -uo pipefail

post_failure_seconds="${1:-10}"
results_directory="${2:-/tmp/4play-two-session-$(date +%Y%m%d-%H%M%S)}"

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
stream_a_port=41301
stream_b_port=41302
input_a_port=42301
input_b_port=42302

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

list_controller_events() {
    local name_path name event_name
    shopt -s nullglob
    for name_path in /sys/class/input/event*/device/name; do
        IFS= read -r name <"$name_path" || continue
        if [[ "$name" == "4-Play Session "*" Player 1" ]]; then
            event_name="$(basename "$(dirname "$(dirname "$name_path")")")"
            printf '/dev/input/%s\n' "$event_name"
        fi
    done
    shopt -u nullglob
}

event_is_known() {
    local candidate="$1"
    shift
    local known
    for known in "$@"; do
        if [[ "$candidate" == "$known" ]]; then
            return 0
        fi
    done
    return 1
}

wait_for_new_event() {
    local timeout_seconds="$1"
    shift
    local candidate

    discovered_event=""
    for ((attempt = 0; attempt < timeout_seconds * 10; attempt++)); do
        while IFS= read -r candidate; do
            if ! event_is_known "$candidate" "$@"; then
                discovered_event="$candidate"
                return 0
            fi
        done < <(list_controller_events)
        sleep 0.1
    done
    return 1
}

last_video_frames() {
    grep -Eo 'video_frames=[0-9]+' "$1" 2>/dev/null | tail -n 1 | cut -d= -f2
}

capture_both_events() {
    local label="$1"
    local destination="$2"
    shift 2
    local monitor_a monitor_b

    timeout 2s stdbuf -oL -eL evtest "$event_a" \
        >"$results_directory/${label}-event-a.log" 2>&1 &
    monitor_a=$!
    timeout 2s stdbuf -oL -eL evtest "$event_b" \
        >"$results_directory/${label}-event-b.log" 2>&1 &
    monitor_b=$!
    sleep 0.25

    "$injector_binary" "$destination" "$@" \
        >"$results_directory/${label}-inject.log" 2>&1
    local injector_status=$?

    wait "$monitor_a" 2>/dev/null || true
    wait "$monitor_b" 2>/dev/null || true
    return "$injector_status"
}

assert_event() {
    local path="$1"
    local pattern="$2"
    local description="$3"
    if grep -Fq "$pattern" "$path"; then
        pass "$description"
    else
        fail "$description"
    fi
}

assert_no_event() {
    local path="$1"
    local pattern="$2"
    local description="$3"
    if grep -Fq "$pattern" "$path"; then
        fail "$description"
    else
        pass "$description"
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

mapfile -t baseline_events < <(list_controller_events)

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
    >"$results_directory/runtime-a.log" 2>&1 &
runtime_a_pid=$!

if wait_for_log "$results_directory/runtime-a.log" "Waiting for seat controller state" 20; then
    pass "session A reached its input loop"
else
    fail "session A reached its input loop"
fi

if wait_for_new_event 10 "${baseline_events[@]}"; then
    event_a="$discovered_event"
    pass "session A created a new virtual input device: $event_a"
else
    fail "session A created a new virtual input device"
fi

known_for_b=("${baseline_events[@]}")
if [[ -n "$event_a" ]]; then
    known_for_b+=("$event_a")
fi

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

if wait_for_new_event 10 "${known_for_b[@]}"; then
    event_b="$discovered_event"
    pass "session B created a second virtual input device: $event_b"
else
    fail "session B created a second virtual input device"
fi

if [[ "$event_a" != "$event_b" && -n "$event_a" && -n "$event_b" ]]; then
    pass "sessions own distinct virtual input devices"
else
    fail "sessions own distinct virtual input devices"
fi

profile_a="/tmp/4play/session-$session_a_id/ctrlr/4play-session.cfg"
profile_b="/tmp/4play/session-$session_b_id/ctrlr/4play-session.cfg"
if [[ -f "$profile_a" ]] && grep -Fq '<mapdevice device=' "$profile_a"; then
    pass "session A generated a MAME controller profile"
else
    fail "session A generated a MAME controller profile"
fi
if [[ -f "$profile_b" ]] && grep -Fq '<mapdevice device=' "$profile_b"; then
    pass "session B generated a MAME controller profile"
else
    fail "session B generated a MAME controller profile"
fi
device_match_a="$(grep -Eo 'device="[0-9a-f]+"' "$profile_a" 2>/dev/null | head -n 1 || true)"
device_match_b="$(grep -Eo 'device="[0-9a-f]+"' "$profile_b" 2>/dev/null | head -n 1 || true)"
if [[ -n "$device_match_a" && -n "$device_match_b" && "$device_match_a" != "$device_match_b" ]]; then
    pass "sessions map different stable device IDs to JOYCODE_1"
else
    fail "sessions map different stable device IDs to JOYCODE_1"
fi

for fifo_path in \
    "/tmp/4play/session-$session_a_id/video.raw" \
    "/tmp/4play/session-$session_a_id/audio.pcm" \
    "/tmp/4play/session-$session_b_id/video.raw" \
    "/tmp/4play/session-$session_b_id/audio.pcm"; do
    if [[ -p "$fifo_path" ]]; then
        pass "session endpoint is an isolated FIFO: $fifo_path"
    else
        fail "session endpoint is an isolated FIFO: $fifo_path"
    fi
done

mame_a_pid="$(pgrep -P "$runtime_a_pid" -x mame | head -n 1 || true)"
mame_b_pid="$(pgrep -P "$runtime_b_pid" -x mame | head -n 1 || true)"
encoder_a_pid="$(pgrep -P "$runtime_a_pid" -x ffmpeg | head -n 1 || true)"
encoder_b_pid="$(pgrep -P "$runtime_b_pid" -x ffmpeg | head -n 1 || true)"

for process_record in \
    "session A MAME:$mame_a_pid" \
    "session B MAME:$mame_b_pid" \
    "session A encoder:$encoder_a_pid" \
    "session B encoder:$encoder_b_pid"; do
    label="${process_record%%:*}"
    pid="${process_record#*:}"
    if [[ -n "$pid" ]] && process_running "$pid"; then
        pass "$label is independently owned (PID $pid)"
    else
        fail "$label is independently owned"
    fi
done

if [[ -n "$mame_a_pid" ]] && tr '\0' ' ' <"/proc/$mame_a_pid/cmdline" | \
    grep -Fq -- "-ctrlrpath /tmp/4play/session-$session_a_id/ctrlr -ctrlr 4play-session"; then
    pass "session A MAME explicitly loads its controller profile"
else
    fail "session A MAME explicitly loads its controller profile"
fi
if [[ -n "$mame_b_pid" ]] && tr '\0' ' ' <"/proc/$mame_b_pid/cmdline" | \
    grep -Fq -- "-ctrlrpath /tmp/4play/session-$session_b_id/ctrlr -ctrlr 4play-session"; then
    pass "session B MAME explicitly loads its controller profile"
else
    fail "session B MAME explicitly loads its controller profile"
fi

if wait_for_log "$results_directory/runtime-a.log" "video_frames=[1-9]" 20; then
    pass "session A is producing video"
else
    fail "session A is producing video"
fi
if wait_for_log "$results_directory/runtime-b.log" "video_frames=[1-9]" 20; then
    pass "session B is producing video"
else
    fail "session B is producing video"
fi

if wait_for_log "$results_directory/receiver-a.log" "320x224" 20; then
    pass "stream A carries TMNT's 320x224 video"
else
    fail "stream A carries TMNT's 320x224 video"
fi
if wait_for_log "$results_directory/receiver-b.log" "288x224" 20; then
    pass "stream B carries Aliens' 288x224 video"
else
    fail "stream B carries Aliens' 288x224 video"
fi

if [[ -n "$event_a" && -n "$event_b" ]] && capture_both_events \
    "route-a" "127.0.0.1:$input_a_port" \
    --buttons action1 --axis-x -1 --hold-ms 300; then
    assert_event "$results_directory/route-a-event-a.log" "BTN_SOUTH), value 1" \
        "session A receives its action-1 press"
    assert_event "$results_directory/route-a-event-a.log" "ABS_X), value -1" \
        "session A receives its left direction"
    assert_no_event "$results_directory/route-a-event-b.log" "BTN_SOUTH), value 1" \
        "session A's action-1 press does not reach session B's device"
    assert_no_event "$results_directory/route-a-event-b.log" "ABS_X), value -1" \
        "session A's left direction does not reach session B's device"
else
    fail "session A input route can be exercised"
fi

if [[ -n "$event_a" && -n "$event_b" ]] && capture_both_events \
    "route-b" "127.0.0.1:$input_b_port" \
    --buttons action6 --axis-x 1 --hold-ms 300; then
    assert_event "$results_directory/route-b-event-b.log" "BTN_TR), value 1" \
        "session B receives its action-6 press"
    assert_event "$results_directory/route-b-event-b.log" "ABS_X), value 1" \
        "session B receives its right direction"
    assert_no_event "$results_directory/route-b-event-a.log" "BTN_TR), value 1" \
        "session B's action-6 press does not reach session A's device"
    assert_no_event "$results_directory/route-b-event-a.log" "ABS_X), value 1" \
        "session B's right direction does not reach session A's device"
else
    fail "session B input route can be exercised"
fi

frames_before="$(last_video_frames "$results_directory/runtime-b.log")"
terminate_process "$runtime_a_pid" 100
runtime_a_status=$last_status
runtime_a_pid=""

if [[ "$runtime_a_status" -eq 0 ]]; then
    pass "session A exits cleanly after SIGTERM"
else
    fail "session A exits cleanly after SIGTERM (status $runtime_a_status)"
fi
if [[ -n "$mame_a_pid" ]] && ! process_running "$mame_a_pid"; then
    pass "session A reaps its MAME child"
else
    fail "session A reaps its MAME child"
fi
if [[ -n "$encoder_a_pid" ]] && ! process_running "$encoder_a_pid"; then
    pass "session A reaps its encoder child"
else
    fail "session A reaps its encoder child"
fi
if [[ -n "$event_a" && ! -e "$event_a" ]]; then
    pass "session A removes its virtual input device"
else
    fail "session A removes its virtual input device"
fi

sleep "$post_failure_seconds"

frames_after="$(last_video_frames "$results_directory/runtime-b.log")"
if [[ -n "$frames_before" && -n "$frames_after" ]] && ((frames_after > frames_before)); then
    pass "session B continues producing video after session A stops ($frames_before -> $frames_after)"
else
    fail "session B continues producing video after session A stops"
fi
if process_running "$runtime_b_pid" && process_running "$mame_b_pid" && process_running "$encoder_b_pid"; then
    pass "session B runtime, MAME, and encoder survive session A shutdown"
else
    fail "session B runtime, MAME, and encoder survive session A shutdown"
fi
if [[ -n "$event_b" && -e "$event_b" ]]; then
    pass "session B virtual input device survives session A shutdown"
else
    fail "session B virtual input device survives session A shutdown"
fi

if [[ -n "$event_b" ]]; then
    timeout 2s stdbuf -oL -eL evtest "$event_b" \
        >"$results_directory/post-failure-event-b.log" 2>&1 &
    post_monitor_pid=$!
    sleep 0.25
    "$injector_binary" "127.0.0.1:$input_b_port" \
        --buttons action2 --axis-y -1 --hold-ms 300 \
        >"$results_directory/post-failure-inject-b.log" 2>&1
    injector_status=$?
    wait "$post_monitor_pid" 2>/dev/null || true
    if [[ "$injector_status" -eq 0 ]]; then
        assert_event "$results_directory/post-failure-event-b.log" "BTN_EAST), value 1" \
            "session B still accepts action input after session A stops"
        assert_event "$results_directory/post-failure-event-b.log" "ABS_Y), value -1" \
            "session B still accepts directional input after session A stops"
    else
        fail "session B input can be injected after session A stops"
    fi
fi

terminate_process "$runtime_b_pid" 100
runtime_b_status=$last_status
runtime_b_pid=""

if [[ "$runtime_b_status" -eq 0 ]]; then
    pass "session B exits cleanly after SIGTERM"
else
    fail "session B exits cleanly after SIGTERM (status $runtime_b_status)"
fi
if [[ -n "$mame_b_pid" ]] && ! process_running "$mame_b_pid"; then
    pass "session B reaps its MAME child"
else
    fail "session B reaps its MAME child"
fi
if [[ -n "$encoder_b_pid" ]] && ! process_running "$encoder_b_pid"; then
    pass "session B reaps its encoder child"
else
    fail "session B reaps its encoder child"
fi
if [[ -n "$event_b" && ! -e "$event_b" ]]; then
    pass "session B removes its virtual input device"
else
    fail "session B removes its virtual input device"
fi

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
