#!/usr/bin/env bash
set -euo pipefail

control_plane_url="${FOURPLAY_CONTROL_PLANE_URL:-http://127.0.0.1:8080}"
seat_api_token="${FOURPLAY_SEAT_API_TOKEN:-phase-1c-seat-token-2026}"
soak_results="${FOURPLAY_PHASE3_SOAK_RESULTS:-}"
results_directory="${FOURPLAY_PHASE3_ACCEPTANCE_RESULTS:-/tmp/4play-phase-3-acceptance-$(date +%Y%m%d-%H%M%S)}"

usage() {
    cat <<EOF
Usage: $(basename "$0") [--soak-results <directory>]

Runs the Phase 3 reference-table acceptance checks that can be automated:
  1. strict systemd smoke
  2. optional soak artifact report

Environment:
  FOURPLAY_CONTROL_PLANE_URL               Default: http://127.0.0.1:8080
  FOURPLAY_SEAT_API_TOKEN                  Default: phase-1c-seat-token-2026
  FOURPLAY_PHASE3_SOAK_RESULTS             Optional soak artifact directory
  FOURPLAY_PHASE3_ACCEPTANCE_RESULTS       Default: /tmp/4play-phase-3-acceptance-<timestamp>
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --soak-results)
            soak_results="${2:-}"
            if [[ -z "$soak_results" ]]; then
                printf '--soak-results requires a directory\n' >&2
                exit 2
            fi
            shift
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

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$results_directory"
summary_json="$results_directory/acceptance-summary.json"
started_at="$(date --iso-8601=seconds)"
git_commit="$(git -C "$repository_root" rev-parse --short HEAD 2>/dev/null || printf 'unknown')"

pass_count=0
fail_count=0

pass() {
    pass_count=$((pass_count + 1))
    printf '[PASS] %s\n' "$1"
}

fail() {
    fail_count=$((fail_count + 1))
    printf '[FAIL] %s\n' "$1"
}

printf 'Phase 3 acceptance results: %s\n' "$results_directory"
printf 'started_at=%s\n' "$started_at"
printf 'git_commit=%s\n' "$git_commit"

if FOURPLAY_CONTROL_PLANE_URL="$control_plane_url" \
    FOURPLAY_SEAT_API_TOKEN="$seat_api_token" \
    FOURPLAY_PHASE3_STRICT_IDLE=1 \
    FOURPLAY_PHASE3_STRICT_METADATA=1 \
    "$repository_root/tools/phase-3-systemd-smoke.sh" \
        >"$results_directory/strict-smoke.txt" \
        2>"$results_directory/strict-smoke.err"; then
    pass "strict systemd smoke"
else
    fail "strict systemd smoke"
fi

if [[ -n "$soak_results" ]]; then
    if "$repository_root/tools/phase-3-soak-report.sh" "$soak_results" \
        >"$results_directory/soak-report.txt" \
        2>"$results_directory/soak-report.err"; then
        pass "soak report"
    else
        fail "soak report"
    fi
else
    printf '[INFO] soak report skipped; pass --soak-results <directory> to include it\n'
fi

python3 - "$summary_json" "$started_at" "$(date --iso-8601=seconds)" "$git_commit" \
    "$control_plane_url" "$soak_results" "$results_directory/strict-smoke.txt" \
    "$results_directory/soak-report.txt" "$pass_count" "$fail_count" <<'PY'
import json
import pathlib
import re
import sys

(
    output_path,
    started_at,
    finished_at,
    git_commit,
    control_plane_url,
    soak_results,
    strict_smoke_path,
    soak_report_path,
    pass_count,
    fail_count,
) = sys.argv[1:]

strict_smoke_summary = None
strict_smoke_text = pathlib.Path(strict_smoke_path)
if strict_smoke_text.exists():
    for line in strict_smoke_text.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("Phase 3 systemd smoke summary:"):
            strict_smoke_summary = line

soak_report = {}
soak_report_text = pathlib.Path(soak_report_path)
if soak_report_text.exists():
    for line in soak_report_text.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" in line and not line.startswith("Phase 3 "):
            key, value = line.split("=", 1)
            if re.fullmatch(r"[a-zA-Z0-9_]+", key):
                soak_report[key] = value

payload = {
    "started_at": started_at,
    "finished_at": finished_at,
    "git_commit": git_commit,
    "control_plane_url": control_plane_url,
    "soak_results": soak_results or None,
    "pass_count": int(pass_count),
    "fail_count": int(fail_count),
    "strict_smoke_summary": strict_smoke_summary,
    "soak_report": soak_report,
}

pathlib.Path(output_path).write_text(
    json.dumps(payload, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY

printf '\nPhase 3 acceptance summary: %s passed, %s failed\n' "$pass_count" "$fail_count"
printf 'Artifacts: %s\n' "$results_directory"
printf 'Summary: %s\n' "$summary_json"

if [[ "$fail_count" -ne 0 ]]; then
    exit 1
fi
