# Native Video Cadence Validation

Date: 2026-10-03

## Problem

FFmpeg received exact-60-Hz TMNT frames with wall-clock timestamps expressed in
the raw-video demuxer's 1/60 time base. That time base was too coarse to
represent the actual arrival times: adjacent frames could receive the same
timestamp, and FFmpeg dropped enough frames to present the stream near 50 fps.
The earlier ten-minute soak encoded 30,404 TMNT frames and reported 5,531
FFmpeg drops.

## Change

The raw-video input now timestamps wall-clock arrivals on a 90 kHz clock. An
FFmpeg `fps` filter then emits frames at the game's configured native rate, and
the output uses passthrough frame synchronization so it does not apply a second
drop/duplicate policy. Audio continues to use wall-clock timestamps.

## Validation

The Linux runtime unit tests passed, followed by sequential one-minute sessions
using `tools/runtime-soak.sh 60 5`. Each runtime exited successfully, cleaned up
its children, finished with empty queues, and dropped only three startup video
frames in the Rust bridge.

| Game | Configured rate | Captured frames | Encoded frames | Encoded duration | Effective encoded rate | A/V start offset |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| TMNT | 60.000000 | 3,739 | 3,737 | 62.22 s | 60.06 fps | +2.488 ms |
| Aliens | 59.185606 | 3,710 | 3,708 | 62.61 s | 59.22 fps | +2.693 ms |
| Killer Instinct | 58.981183 | 3,695 | 3,694 | 62.59 s | 59.02 fps | +2.429 ms |

TMNT no longer falls to approximately 50 fps, while the two non-integer-rate
profiles retain their native cadence. A live-play check remains useful for
confirming the subjective latency and synchronization result after deployment.
