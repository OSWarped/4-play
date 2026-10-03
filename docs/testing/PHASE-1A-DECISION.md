# Phase 1A Decision

## Status

**Decision: revise.** Continue the direct seat-to-runtime architecture and
targeted runtime hardening; do not pivot. Phase 1A is not closed because
objective latency measurement is deferred and the exact-60-Hz TMNT profile is
presented by FFmpeg at approximately 50 fps under wall-clock timestamping.

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

## Evidence still missing

- local and remote button-to-photon distributions
- objective local and streamed action-to-sound timing
- rendered Windows-player resource and memory-plateau measurements
- full two-session controller and failure isolation
- native-rate presentation for the exact-60-Hz TMNT profile

## Provisional interpretation

The direct seat-to-runtime architecture remains the leading approach. Tight
subjective controls, synchronized sound, bounded long-running behavior, clean
shutdown, and modest single-session resource use justify continued runtime
engineering. Missing objective latency data and TMNT's presentation cadence do
not justify broad control-plane or Product MVP development yet.

## Next decision gate

Complete the following in order:

1. correct or explicitly accept the exact-60-Hz presentation cadence
2. measure local and remote input-to-video latency when capture equipment is
   available
3. objectively confirm local and streamed action-to-sound timing
4. run a longer rendered Windows-player resource test
5. update this document and reconsider a `go` decision

## Final decision

**Revise.** The architecture is viable and should continue. Phase 1A remains
open for presentation-cadence correction and objective measurement; a pivot is
not warranted.
