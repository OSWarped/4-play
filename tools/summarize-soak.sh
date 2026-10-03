#!/usr/bin/env bash

set -euo pipefail

results_directory="${1:-}"

if [[ -z "$results_directory" || ! -d "$results_directory" ]]; then
    echo "usage: $0 <soak-results-directory>" >&2
    exit 2
fi

printf 'game\tprocess\tsamples\tavg_cpu_percent\tmax_cpu_percent\tmax_rss_mib\n'

for resources_path in "$results_directory"/*-resources.tsv; do
    game="$(basename "$resources_path" -resources.tsv)"

    awk -F '\t' -v game="$game" '
        NR > 1 {
            process = $4
            samples[process]++
            cpu_total[process] += $5
            if ($5 > cpu_max[process]) {
                cpu_max[process] = $5
            }
            if ($6 > rss_max[process]) {
                rss_max[process] = $6
            }
        }
        END {
            for (process in samples) {
                printf "%s\t%s\t%d\t%.2f\t%.2f\t%.2f\n",
                    game,
                    process,
                    samples[process],
                    cpu_total[process] / samples[process],
                    cpu_max[process],
                    rss_max[process] / 1024.0
            }
        }
    ' "$resources_path"
done | sort -k1,1 -k2,2

echo
cat "$results_directory/summary.tsv"
