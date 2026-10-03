# Current Implementation Status

Last updated: 2026-10-03

## Working now

- Cargo workspace builds successfully
- Rust session runtime prepares isolated session resources
- Rust launches and owns FFmpeg
- Rust launches and owns headless MAME
- MAME emits raw BGR0 video and raw PCM audio
- Rust bridges both media streams concurrently
- bounded queues prevent unbounded media buffering
- FFmpeg sends H.264/AAC over UDP MPEG-TS
- Windows VLC receives the live stream
- audio/video synchronization is validated across multiple titles
- CHD-backed MAME content is validated
- two media sessions can run simultaneously
- Ctrl+C leaves no observed MAME or FFmpeg process running
- Rust can create and drive a Linux virtual controller
- MAME consumes a runtime-owned virtual controller
- the Windows `seat-input` client sends complete controller-state snapshots over UDP
- real key-down/key-up, diagonals, movement plus action, and simultaneous buttons work
- sequence checks discard stale input packets
- a 250 ms input timeout neutralizes controls and permits reconnection
- low-buffer FFplay playback provides subjectively excellent control-to-video response

## In progress

- measure local and remote button-to-photon latency objectively
- isolate a noticeable streamed audio delay from the responsive video path
- compare AAC/20 ms PCM blocks with low-delay Opus/5 ms PCM blocks
- test whether the 64-packet FFmpeg raw-audio input queue accumulates game audio
- replace manual width, height, and refresh arguments with MAME metadata discovery
- wire the session state machine into actual runtime transitions
- improve cleanup of stale session FIFOs and directories

## Not started

- objective button-to-photon latency measurement
- control-plane service
- runtime-host registration
- catalog and game-package workflow
- active-session discovery and previews
- player-slot reservation
- spectator and join workflows

## Next milestone

Record repeatable local and remote input-to-video distributions and characterize
the remaining audio delay without adding latency to the proven video path.
