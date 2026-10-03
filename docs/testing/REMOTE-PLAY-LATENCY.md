# Remote-Play Latency and Consistency

## Status

Not yet measured objectively. Subjective play tests completed in August and
October 2026 found highly responsive control-to-video behavior with low-buffer
FFplay. Streamed audio remained noticeably behind visible action.

## What is already known

- synchronized live audio/video reaches the Windows seat
- the media bridge can sustain real-time game frame rates
- queue depth is bounded
- stale video can be discarded instead of accumulated
- simultaneous sessions have run without observed media cross-talk
- per-game refresh metadata is required for correct timing
- MAME consumes the runtime-owned virtual controller
- complete controller state reaches the runtime directly from Windows
- diagonals and simultaneous action combinations work
- controls neutralize after 250 ms without valid input packets
- FFplay's low-buffer profile feels substantially more responsive than buffered playback
- forcing FFplay to use `-sync video` caused unacceptable presentation latency and is rejected

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

## Subjective results

The current low-latency reference receiver is:

```powershell
ffplay -f mpegts `
  -fflags nobuffer `
  -flags low_delay `
  -framedrop `
  -probesize 32768 `
  -analyzeduration 0 `
  "udp://0.0.0.0:41008?fifo_size=1000000&overrun_nonfatal=1"
```

Observed:

- input-to-video response feels excellent
- simultaneous controls behave correctly
- audio follows visible action by a noticeable subjective interval
- VLC with 50 ms network caching is stable but retains modest audio delay
- VLC with 20 ms caching is less stable and does not improve perceived synchronization
- `ffplay -sync video` delays the visible game and is unsuitable
- a synchronized synthetic flash/beep remained precisely aligned through AAC,
  MPEG-TS, the wired network, FFplay, Windows, and HDMI audio
- a five-minute game run started raw video and audio only 2.29 ms apart and
  sustained 60.01 fps, but independently discarded 254 video frames and 179
  20 ms audio blocks; those losses shorten the video timeline about 650 ms
  more than the audio timeline and explain the observed audio lag

These observations are not substitutes for camera-based measurements.

## Audio experiment matrix

The synchronized synthetic test substantially reduces the likelihood that AAC,
MPEG-TS, FFplay, Windows, or HDMI output creates the game-only delay. Runtime
metrics then identified independent media dropping as the leading cause. FFmpeg
timestamps the raw inputs from the frame and sample counts it receives, so
discarding unequal durations permanently moves one media timeline ahead of the
other.

Compare in this order:

| Profile | Runtime options | Purpose |
| --- | --- | --- |
| Baseline | `--audio-codec aac --audio-block-ms 20 --audio-thread-queue-size 64` | Reproduce the current path |
| Reduced queue | `--audio-codec aac --audio-block-ms 20 --audio-thread-queue-size 4` | Test FFmpeg raw-input backlog |
| Small block | `--audio-codec aac --audio-block-ms 5 --audio-thread-queue-size 4` | Isolate bridge block contribution |
| Opus | `--audio-codec opus --audio-block-ms 5 --audio-thread-queue-size 4` | Secondary codec comparison |
| Wall-clock timestamps | AAC defaults with drop-oldest bridge queues | Preserve elapsed time across unequal drops; current leading fix |

For every run, record runtime `video_start_ms`, `audio_start_ms`, `offset_ms`,
queue depths, dropped blocks, perceived synchronization, and playback stability.

The synthetic test result makes Opus a secondary experiment rather than the
leading fix. AAC remained synchronized when the MAME/raw-PCM bridge was absent.
The small-block and Opus tests are deferred until wall-clock timestamps are
tested. A lossless bounded bridge was rejected because FFmpeg opens its two raw
inputs sequentially while MAME writes both FIFOs from one execution path;
backpressure on the unopened input deadlocked startup. The bridge therefore
retains drop-oldest behavior for responsiveness, while FFmpeg timestamps both
raw inputs from wall-clock time so unequal drops preserve elapsed time. Both
formats also use the minimum probe size and a one-microsecond analysis ceiling,
and redundant FPS probing is disabled for the declared raw video rate.

## Interpretation

Remote input is implemented. The Phase 1A decision now depends on objective
latency distributions and whether audio can be improved without compromising
the responsive video path.
