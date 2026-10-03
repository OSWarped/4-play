# Phase 1A Decision

## Status

Decision deferred pending objective latency measurements. Remote input and
responsive gameplay are now demonstrated, so the remaining decision evidence
is measurement and audio characterization rather than basic feasibility.

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

## Evidence still missing

- local and remote button-to-photon distributions
- objective local and streamed action-to-sound timing
- complete reference-host resource measurements
- full two-session controller and failure isolation

## Provisional interpretation

The direct seat-to-runtime architecture remains the leading approach. The current results justify continuing the experiment, but they do not justify beginning broad control-plane or Product MVP development yet.

## Next decision gate

Complete the following in order:

1. reproduce the responsive FFplay baseline
2. measure local and remote input-to-video latency
3. compare local and streamed action-to-sound timing
4. compare the AAC and low-delay Opus experiment profiles
5. record host and client resource use
6. update this document with a go, revise, or pivot decision

## Final decision

Pending.
