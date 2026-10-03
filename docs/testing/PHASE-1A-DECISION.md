# Phase 1A Decision

## Status

Decision deferred pending objective latency measurements. Remote input and
responsive gameplay are now demonstrated, and the streamed action-to-sound lag
has been resolved subjectively. The remaining decision evidence is objective
measurement and resource characterization rather than basic feasibility.

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

## Evidence still missing

- local and remote button-to-photon distributions
- objective local and streamed action-to-sound timing
- complete reference-host resource measurements
- full two-session controller and failure isolation

## Provisional interpretation

The direct seat-to-runtime architecture remains the leading approach. The current results justify continuing the experiment, but they do not justify beginning broad control-plane or Product MVP development yet.

## Next decision gate

Complete the following in order:

1. measure local and remote input-to-video latency
2. objectively confirm local and streamed action-to-sound timing
3. record host and client resource use
4. update this document with a go, revise, or pivot decision

## Final decision

Pending.
