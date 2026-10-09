#!/usr/bin/env python3
"""Compare two Phase 3 acceptance-summary.json artifacts."""

from __future__ import annotations

import json
import sys
from pathlib import Path


def usage() -> None:
    print(
        "usage: phase-3-compare-acceptance.py <baseline-summary.json> <candidate-summary.json>",
        file=sys.stderr,
    )


def load(path: str) -> dict:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def main() -> int:
    if len(sys.argv) != 3:
        usage()
        return 2

    baseline = load(sys.argv[1])
    candidate = load(sys.argv[2])
    failures: list[str] = []

    candidate_fail_count = int(candidate.get("fail_count", 0))
    if candidate_fail_count != 0:
        failures.append(f"candidate has fail_count={candidate_fail_count}")

    baseline_smoke = baseline.get("strict_smoke_summary")
    candidate_smoke = candidate.get("strict_smoke_summary")
    if baseline_smoke != candidate_smoke:
        failures.append(
            f"strict smoke summary changed: baseline={baseline_smoke!r} candidate={candidate_smoke!r}"
        )

    baseline_soak = baseline.get("soak_report") or {}
    candidate_soak = candidate.get("soak_report") or {}
    for key in ("sample_error_files", "diagnostics_errors"):
        baseline_value = baseline_soak.get(key)
        candidate_value = candidate_soak.get(key)
        if baseline_value != candidate_value:
            failures.append(
                f"soak {key} changed: baseline={baseline_value!r} candidate={candidate_value!r}"
            )

    print(f"baseline={sys.argv[1]}")
    print(f"candidate={sys.argv[2]}")
    print(f"baseline_commit={baseline.get('git_commit')}")
    print(f"candidate_commit={candidate.get('git_commit')}")
    print(f"candidate_pass_count={candidate.get('pass_count')}")
    print(f"candidate_fail_count={candidate.get('fail_count')}")

    if failures:
        print("comparison=failed")
        for failure in failures:
            print(f"  {failure}")
        return 1

    print("comparison=passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
