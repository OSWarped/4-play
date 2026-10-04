# Current Implementation Status

Last updated: 2026-10-04

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
- SIGINT and SIGTERM request orderly runtime shutdown; MAME and FFmpeg are
  killed when necessary and reaped before the runtime exits
- Rust can create and drive a Linux virtual controller
- MAME consumes a runtime-owned virtual controller
- the Windows `seat-input` client sends complete controller-state snapshots over UDP
- real key-down/key-up, diagonals, movement plus action, and simultaneous buttons work
- sequence checks discard stale input packets
- a 250 ms input timeout neutralizes controls and permits reconnection
- low-buffer FFplay playback provides subjectively excellent control-to-video response
- runtime metrics traced the game-only audio lag to unequal independent bridge
  drops: 254 video frames versus 179 20 ms audio blocks produced approximately
  650 ms of relative timeline skew
- the runtime preserves real elapsed time with wall-clock input
  timestamps when its bounded low-latency queues discard stale media
- live TMNT gameplay validated the result subjectively: controls remained tight
  and game sound was synchronized with visible action
- a thirty-minute sequential TMNT/Aliens/Killer Instinct soak completed with
  exit status 0, clean child-process cleanup, stable resources, empty final
  queues, and no audio drops for all three sessions
- host resource use is recorded from sixty samples per process per game; a
  Windows decoder-only benchmark establishes a low client resource floor
- a 90 kHz raw-video timestamp clock plus native-rate filtering preserves
  wall-clock synchronization while presenting exact-60-Hz TMNT at 60.06 fps
- one-minute TMNT, Aliens, and Killer Instinct regression sessions encoded
  essentially one frame for every captured frame at each configured native rate
- an automated two-session isolation harness proved distinct media endpoints,
  streams, processes, virtual controllers, and UDP input routes
- orderly termination of one concurrent session left the other session's
  stream and controller input operating normally
- virtual controllers now have session-specific identities, and each MAME
  process explicitly loads a generated profile that maps only its assigned
  device to `JOYCODE_1`

## In progress

- measure local and remote button-to-photon latency objectively
- complete a rendered Windows-player resource soak; the decoder-only working
  set rose from 27.67 MiB to 50.94 MiB during a 168-second sample
- visually confirm that two concurrent MAME processes consume only their own
  virtual controllers
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

Record repeatable local and remote input-to-video distributions and objectively
confirm the subjectively synchronized action-to-sound result.
