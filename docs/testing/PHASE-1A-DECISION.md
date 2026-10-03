# Phase 1A Decision

## Status

**Decision: revise.** Continue the direct seat-to-runtime architecture and
targeted runtime hardening; do not pivot. Phase 1A is not closed because
objective latency measurement is deferred and a rendered Windows-player
resource soak is still outstanding.

## Evidence available

- headless MAME launch works
- Rust owns MAME and FFmpeg from one command
- synchronized live media reaches a remote Windows seat
- multiple games with different resolutions and refresh rates work
- CHD-backed content works
- two media sessions can run simultaneously
- Linux virtual-controller creation and generated events work
- MAME consumes the runtime-owned virtual controller
- a Windows client delivers complete controller state directly to the runtime
- simultaneous movement and action inputs work
- timeout neutralization and client reconnection work
- low-buffer FFplay playback feels highly responsive in repeated play tests
- a synthetic synchronized flash/beep remained aligned through the downstream
  AAC/MPEG-TS/network/player/audio-output path
- runtime metrics traced the game-only sound lag to unequal media drops that
  shortened the video timeline roughly 650 ms more than the audio timeline
- wall-clock timestamps preserve elapsed time across unequal queue drops
- live TMNT play confirmed synchronized sound and action while controls remained
  subjectively immediate
- a thirty-minute sequential TMNT/Aliens/Killer Instinct soak completed with
  exit status 0, clean cleanup, stable resources, empty final queues, and zero
  audio drops for every session
- one-session host use ranged from about 47% to 60% of one CPU and 344–383 MiB
  summed maximum RSS across runtime, FFmpeg, and MAME
- a Windows decoder-only TMNT benchmark averaged 0.44% of one CPU and 39.62 MiB
  working set at approximately 1.40 Mbit/s
- high-resolution wall-clock video timestamps followed by native-rate filtering
  corrected TMNT's approximately 50 fps presentation to 60.06 fps without
  changing the wall-clock audio synchronization strategy
- one-minute regression sessions encoded 3,737 of 3,739 TMNT frames, 3,708 of
  3,710 Aliens frames, and 3,694 of 3,695 Killer Instinct frames while retaining
  each game's configured cadence

## Evidence still missing

- local and remote button-to-photon distributions
- objective local and streamed action-to-sound timing
- rendered Windows-player resource and memory-plateau measurements
- full two-session controller and failure isolation

## Provisional interpretation

The direct seat-to-runtime architecture remains the leading approach. Tight
subjective controls, synchronized sound, native video cadence, bounded
long-running behavior, clean shutdown, and modest single-session resource use
justify continued runtime engineering. Missing objective latency and rendered
client resource data do not justify broad control-plane or Product MVP
development yet.

## Next decision gate

Complete the following in order:

1. measure local and remote input-to-video latency when capture equipment is
   available
2. objectively confirm local and streamed action-to-sound timing
3. run a longer rendered Windows-player resource test
4. update this document and reconsider a `go` decision

## Final decision

**Revise.** The architecture is viable and should continue. Phase 1A remains
open for objective measurement and rendered-player resource validation; a
pivot is not warranted.
