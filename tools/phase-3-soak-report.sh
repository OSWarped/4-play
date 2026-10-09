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

if [[ ! -f "$summary_path" ]]; then
    printf 'summary.tsv not found in %s\n' "$results_directory" >&2
    exit 2
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
    error_count="$(
        find "$samples_directory" -type f -name '*.err' -print0 \
            | xargs -0 grep -L '^$' 2>/dev/null \
            | wc -l
    )"
    printf 'sample_error_files=%s\n' "$error_count"
    if [[ "$error_count" != "0" ]]; then
        printf 'nonempty sample error files:\n'
        find "$samples_directory" -type f -name '*.err' -print0 \
            | xargs -0 grep -L '^$' 2>/dev/null \
            | sed 's/^/  /'
    fi
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
