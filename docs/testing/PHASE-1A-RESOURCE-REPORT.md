# Phase 1A Resource Report

## Status

Recorded on October 3, 2026 from the reference Linux runtime host and Windows
seat. Host values aggregate sixty ten-second samples per process per game. The
Windows result is a decoder-only lower bound and does not include SDL rendering
or speaker output.

## Runtime host

| Game | Process | Average CPU | Maximum CPU | Maximum RSS |
| --- | --- | ---: | ---: | ---: |
| TMNT | session-runtime | 4.82% | 4.90% | 3.88 MiB |
| TMNT | FFmpeg | 15.05% | 15.60% | 59.65 MiB |
| TMNT | MAME | 26.83% | 34.80% | 288.75 MiB |
| Aliens | session-runtime | 4.53% | 4.60% | 3.46 MiB |
| Aliens | FFmpeg | 15.35% | 16.10% | 58.89 MiB |
| Aliens | MAME | 27.06% | 34.20% | 281.82 MiB |
| Killer Instinct | session-runtime | 4.78% | 4.90% | 3.55 MiB |
| Killer Instinct | FFmpeg | 14.00% | 15.50% | 61.68 MiB |
| Killer Instinct | MAME | 41.17% | 42.30% | 317.38 MiB |

Approximate summed average CPU was 46.7% of one core for TMNT, 46.9% for
Aliens, and 60.0% for Killer Instinct. Summed per-process maximum RSS was about
352 MiB, 344 MiB, and 383 MiB respectively. These are planning observations,
not multi-session capacity limits.

Final encoded MPEG-TS bitrates were approximately 1.30 Mbit/s for TMNT,
1.94 Mbit/s for Aliens, and 1.32 Mbit/s for Killer Instinct. Network framing
overhead is not included.

## Windows seat lower bound

A 167.9-second TMNT stream was received by FFmpeg 9.0 on Windows, decoded, and
discarded to the null output. Twelve ten-second samples produced:

- average decoder CPU: 0.44% of one CPU
- maximum decoder CPU: 1.40%
- average working set: 39.62 MiB
- maximum working set: 50.94 MiB
- received encoded bitrate: approximately 1.40 Mbit/s

Working set increased from 27.67 MiB to 50.94 MiB during this short run. A
longer rendered FFplay test is needed to determine whether memory plateaus and
to include presentation and audio-output costs.

## Interpretation

One session is inexpensive on both reference systems. Killer Instinct confirms
that MAME, rather than encoding or runtime coordination, is the dominant host
CPU and memory cost. The results support continued single-session development
but do not yet establish safe multi-session density.
