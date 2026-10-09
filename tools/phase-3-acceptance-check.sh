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

printf '\nPhase 3 acceptance summary: %s passed, %s failed\n' "$pass_count" "$fail_count"
printf 'Artifacts: %s\n' "$results_directory"

if [[ "$fail_count" -ne 0 ]]; then
    exit 1
fi
