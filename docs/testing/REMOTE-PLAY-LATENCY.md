# Remote-Play Latency and Consistency

## Status

Not yet measured. This document defines the evidence still required before Phase 1A can be closed.

## What is already known

- synchronized live audio/video reaches the Windows seat
- the media bridge can sustain real-time game frame rates
- queue depth is bounded
- stale video can be discarded instead of accumulated
- simultaneous sessions have run without observed media cross-talk
- per-game refresh metadata is required for correct timing

These observations are useful but do not measure button-to-photon latency.

## Required test path

```text
seat input device
  → seat input client
  → wired LAN
  → session runtime
  → virtual controller
  → MAME
  → raw video
  → Rust bridge
  → FFmpeg
  → wired LAN
  → seat decoder and display
```

## Prerequisites

- runtime-created virtual controller is consumed by MAME
- remote seat input transport is working
- input timeout releases buttons and centers axes
- the same representative game and input action can be repeated locally and remotely

## Metrics

Record:

- local median button-to-photon latency
- remote median button-to-photon latency
- local and remote 95th percentile
- local and remote 99th percentile when sample count permits
- added remote latency
- jitter or spread
- input packet loss, duplication, lateness, and ordering
- stuck-input incidents
- dropped or repeated video frames
- observed A/V synchronization
- host and seat resource use

## Provisional goals

- less than 30 ms added median latency
- less than 50 ms added 95th-percentile latency
- no recurring input-loss or stuck-input behavior

These remain experiment goals rather than universal product requirements.

## Suggested method

Use a high-frame-rate camera that can see both the physical input action and the display response. Capture enough repeated samples to calculate a distribution rather than one anecdotal result. Document camera frame rate, display refresh rate, game state, input action, sample exclusions, and measurement uncertainty.

## Results

Pending.

## Interpretation

Pending remote input implementation and measurement.
