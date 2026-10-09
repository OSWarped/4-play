#!/usr/bin/env bash
set -euo pipefail

results_directory="${1:-}"

if [[ -z "$results_directory" || ! -d "$results_directory" ]]; then
    printf 'usage: %s <phase-3-soak-results-directory>\n' "$(basename "$0")" >&2
    exit 2
fi

summary_path="$results_directory/summary.tsv"
samples_directory="$results_directory/samples"
strict_smoke_path="$results_directory/strict-smoke.txt"
diagnostics_err_path="$results_directory/diagnostics.err"

write_summary_from_samples() {
    python3 - "$results_directory" "$summary_path" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
summary_path = pathlib.Path(sys.argv[2])
samples = sorted((root / "samples").glob("*-sessions.json"))
terminal_states = {"stopped", "allocation_failed", "launch_failed", "runtime_lost", "terminated"}

with summary_path.open("w", encoding="utf-8") as output:
    print("sample\tactive_sessions\tactive_states\tterminal_history\truntime_hosts", file=output)
    for sessions_path in samples:
        sample_id = sessions_path.name.split("-", 1)[0]
        hosts_path = sessions_path.with_name(f"{sample_id}-runtime-hosts.json")
        active_count = 0
        active_states = "none"
        terminal_history = "none"
        try:
            sessions = json.loads(sessions_path.read_text(encoding="utf-8")).get("sessions", [])
        except Exception as error:
            active_states = f"sessions_error:{error}"
        else:
            active = [
                session for session in sessions
                if session.get("state") not in terminal_states
            ]
            active_count = len(active)
            active_state_counts = {}
            terminal_state_counts = {}
            for session in active:
                state = session.get("state", "unknown")
                active_state_counts[state] = active_state_counts.get(state, 0) + 1
            for session in sessions:
                state = session.get("state", "unknown")
                if state in terminal_states:
                    terminal_state_counts[state] = terminal_state_counts.get(state, 0) + 1
            active_states = ",".join(
                f"{state}:{count}" for state, count in sorted(active_state_counts.items())
            ) or "none"
            terminal_history = ",".join(
                f"{state}:{count}" for state, count in sorted(terminal_state_counts.items())
            ) or "none"
        try:
            host_payload = json.loads(hosts_path.read_text(encoding="utf-8"))
            hosts = host_payload.get("hosts", host_payload.get("runtime_hosts", []))
        except Exception as error:
            host_summary = f"hosts_error:{error}"
        else:
            host_summary = ",".join(
                f"{host.get('id')}:{host.get('status')}" for host in hosts
            ) or "none"
        print(
            f"{sample_id}\t{active_count}\t{active_states}\t{terminal_history}\t{host_summary}",
            file=output,
        )
PY
}

if [[ ! -f "$summary_path" ]] || [[ "$(wc -l <"$summary_path")" -le 1 ]]; then
    if [[ -d "$samples_directory" ]]; then
        write_summary_from_samples
    else
        printf 'summary.tsv and samples directory not found in %s\n' "$results_directory" >&2
        exit 2
    fi
fi

printf 'Phase 3 soak report: %s\n' "$results_directory"

python3 - "$summary_path" <<'PY'
import csv
import sys

path = sys.argv[1]
with open(path, encoding="utf-8", newline="") as handle:
    rows = list(csv.DictReader(handle, delimiter="\t"))

if not rows:
    print("samples=0")
    print("max_active_sessions=0")
    print("host_statuses=none")
    raise SystemExit

max_active = 0
host_statuses = {}
active_state_counts = {}
for row in rows:
    try:
        max_active = max(max_active, int(row.get("active_sessions", "0")))
    except ValueError:
        pass
    for host in row.get("runtime_hosts", "none").split(","):
        if not host or host == "none":
            continue
        host_statuses[host] = host_statuses.get(host, 0) + 1
    for state in row.get("active_states", "none").split(","):
        if not state or state == "none":
            continue
        active_state_counts[state] = active_state_counts.get(state, 0) + 1

print(f"samples={len(rows)}")
print(f"max_active_sessions={max_active}")
print(
    "host_statuses="
    + (",".join(f"{status}:{count}" for status, count in sorted(host_statuses.items())) or "none")
)
print(
    "active_state_samples="
    + (",".join(f"{state}:{count}" for state, count in sorted(active_state_counts.items())) or "none")
)
PY

if [[ -d "$samples_directory" ]]; then
    nonempty_error_list="$(mktemp)"
    find "$samples_directory" -type f -name '*.err' -size +0c -print \
        >"$nonempty_error_list"
    error_count="$(wc -l <"$nonempty_error_list")"
    printf 'sample_error_files=%s\n' "$error_count"
    if [[ "$error_count" != "0" ]]; then
        printf 'nonempty sample error files:\n'
        sed 's/^/  /' "$nonempty_error_list"
    fi
    rm -f "$nonempty_error_list"
fi

if [[ -f "$diagnostics_err_path" && -s "$diagnostics_err_path" ]]; then
    printf 'diagnostics_errors=present\n'
else
    printf 'diagnostics_errors=none\n'
fi

if [[ -f "$strict_smoke_path" ]]; then
    grep 'Phase 3 systemd smoke summary:' "$strict_smoke_path" || true
else
    printf 'strict_smoke=not_run\n'
fi
