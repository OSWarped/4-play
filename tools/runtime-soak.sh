#!/usr/bin/env bash

set -uo pipefail

duration_seconds="${1:-600}"
sample_seconds="${2:-10}"
destination_ip="${3:-127.0.0.1}"
results_directory="${4:-/tmp/4play-soak-$(date +%Y%m%d-%H%M%S)}"

if ! [[ "$duration_seconds" =~ ^[1-9][0-9]*$ ]]; then
    echo "duration must be a positive integer" >&2
    exit 2
fi

if ! [[ "$sample_seconds" =~ ^[1-9][0-9]*$ ]]; then
    echo "sample interval must be a positive integer" >&2
    exit 2
fi

script_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_directory/.." && pwd)"
runtime_binary="$repository_root/target/debug/session-runtime"

if [[ ! -x "$runtime_binary" ]]; then
    echo "runtime binary not found: $runtime_binary" >&2
    exit 2
fi

mkdir -p "$results_directory"
summary_path="$results_directory/summary.tsv"
printf 'game\truntime_status\tcleanup\tlast_metric\n' >"$summary_path"

active_runtime_pid=""

stop_active_runtime() {
    if [[ -z "$active_runtime_pid" ]] || ! kill -0 "$active_runtime_pid" 2>/dev/null; then
        return
    fi

    kill -TERM "$active_runtime_pid" 2>/dev/null || true

    for _ in $(seq 1 50); do
        if ! kill -0 "$active_runtime_pid" 2>/dev/null; then
            return
        fi
        sleep 0.1
    done

    kill -KILL "$active_runtime_pid" 2>/dev/null || true
}

trap stop_active_runtime EXIT INT TERM

run_game() {
    local label="$1"
    local rom="$2"
    local width="$3"
    local height="$4"
    local fps="$5"
    local index="$6"

    local stream_port=$((41100 + index))
    local input_port=$((42100 + index))
    local session_id=$((200 + index))
    local runtime_log="$results_directory/$label-runtime.log"
    local resources_log="$results_directory/$label-resources.tsv"

    printf 'timestamp\tpid\tppid\tname\tcpu_percent\trss_kib\tmetric\n' >"$resources_log"

    setsid "$runtime_binary" \
        --session-id "$session_id" \
        --rom "$rom" \
        --width "$width" \
        --height "$height" \
        --fps "$fps" \
        --destination-ip "$destination_ip" \
        --udp-port "$stream_port" \
        --input-port "$input_port" \
        --audio-codec aac \
        --audio-block-ms 20 \
        --audio-thread-queue-size 4 \
        >"$runtime_log" 2>&1 &

    active_runtime_pid=$!
    local runtime_pid="$active_runtime_pid"
    local started_at
    started_at=$(date +%s)
    local ffmpeg_pid=""
    local mame_pid=""

    sleep 3
    ffmpeg_pid=$(pgrep -P "$runtime_pid" ffmpeg | head -n 1 || true)
    mame_pid=$(pgrep -P "$runtime_pid" mame | head -n 1 || true)

    while kill -0 "$runtime_pid" 2>/dev/null; do
        local now elapsed metric timestamp
        now=$(date +%s)
        elapsed=$((now - started_at))

        if ((elapsed >= duration_seconds)); then
            break
        fi

        timestamp=$(date --iso-8601=seconds)
        metric=$(grep 'video_frames=' "$runtime_log" | tail -n 1 | tr '\t' ' ' || true)

        for pid in "$runtime_pid" "$ffmpeg_pid" "$mame_pid"; do
            if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
                ps -p "$pid" -o pid=,ppid=,comm=,%cpu=,rss= | awk \
                    -v timestamp="$timestamp" -v metric="$metric" \
                    '{ printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\n", timestamp, $1, $2, $3, $4, $5, metric }' \
                    >>"$resources_log"
            fi
        done

        sleep "$sample_seconds"
    done

    kill -TERM "$runtime_pid" 2>/dev/null || true

    for _ in $(seq 1 100); do
        if ! kill -0 "$runtime_pid" 2>/dev/null; then
            break
        fi
        sleep 0.1
    done

    local runtime_status=0
    if kill -0 "$runtime_pid" 2>/dev/null; then
        kill -KILL "$runtime_pid" 2>/dev/null || true
        runtime_status=137
    else
        wait "$runtime_pid" || runtime_status=$?
    fi

    local cleanup_status="clean"
    for pid in "$runtime_pid" "$ffmpeg_pid" "$mame_pid"; do
        if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
            cleanup_status="orphaned"
            kill -KILL "$pid" 2>/dev/null || true
        fi
    done

    local last_metric
    last_metric=$(grep 'video_frames=' "$runtime_log" | tail -n 1 | tr '\t' ' ' || true)
    printf '%s\t%s\t%s\t%s\n' \
        "$label" "$runtime_status" "$cleanup_status" "$last_metric" \
        >>"$summary_path"

    active_runtime_pid=""
}

run_game "tmnt" "tmnt" 320 224 60.000000 1
run_game "aliens" "aliens" 288 224 59.185606 2
run_game "kinst" "kinst" 320 240 58.981183 3

echo "Soak results: $results_directory"
