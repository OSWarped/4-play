#!/usr/bin/env bash
set -euo pipefail

acceptance_summary="${1:-}"
output_path="${2:-}"

usage() {
    cat <<EOF
Usage: $(basename "$0") <acceptance-summary.json> [output-path]

Creates a dated Phase 3 closeout note from the checked-in closeout template.
If output-path is omitted, the note is written to:
  docs/testing/PHASE-3-CLOSEOUT-<YYYYMMDD>.md
EOF
}

if [[ -z "$acceptance_summary" || "$acceptance_summary" == "--help" || "$acceptance_summary" == "-h" ]]; then
    usage
    exit 2
fi

if [[ ! -f "$acceptance_summary" ]]; then
    printf 'acceptance summary not found: %s\n' "$acceptance_summary" >&2
    exit 2
fi

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
template_path="$repository_root/docs/testing/PHASE-3-CLOSEOUT-TEMPLATE.md"

if [[ ! -f "$template_path" ]]; then
    printf 'closeout template not found: %s\n' "$template_path" >&2
    exit 2
fi

if [[ -z "$output_path" ]]; then
    output_path="$repository_root/docs/testing/PHASE-3-CLOSEOUT-$(date +%Y%m%d).md"
fi

if [[ -e "$output_path" ]]; then
    printf 'output already exists: %s\n' "$output_path" >&2
    exit 2
fi

python3 - "$acceptance_summary" "$template_path" "$output_path" <<'PY'
import json
import pathlib
import sys

summary_path = pathlib.Path(sys.argv[1])
template_path = pathlib.Path(sys.argv[2])
output_path = pathlib.Path(sys.argv[3])

summary = json.loads(summary_path.read_text(encoding="utf-8"))
template = template_path.read_text(encoding="utf-8")

content = template.replace(
    "Status: **accepted / accepted with follow-ups / not accepted**",
    "Status: **draft — physical validation pending**",
)
content = content.replace("Date:\n", f"Date: {summary.get('finished_at', '')}\n")
content = content.replace(
    "| Acceptance summary JSON | |",
    f"| Acceptance summary JSON | `{summary_path}` |",
)
content = content.replace(
    "| Soak artifact directory | |",
    f"| Soak artifact directory | `{summary.get('soak_results') or ''}` |",
)
content = content.replace(
    "| Final strict smoke artifact | |",
    f"| Final strict smoke artifact | `{summary.get('strict_smoke_summary') or ''}` |",
)

output_path.write_text(content, encoding="utf-8")
PY

printf 'Created Phase 3 closeout draft: %s\n' "$output_path"
