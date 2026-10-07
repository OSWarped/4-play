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
- manual TMNT and Killer Instinct play confirmed end-to-end controller
  isolation between two simultaneous sessions
- abnormal MAME and FFmpeg child failures terminate only the owning runtime,
  return a failure status, and clean the remaining child, virtual controller,
  input port, and transient media FIFOs
- a second session continues streaming and accepting input while its peer
  fails, and the failed session can restart on the same ID and ports
- normal runtime shutdown requests a graceful MAME exit and can flush MAME
  autosave state before child-process cleanup
- a 39-assertion concurrent persistence test proved distinct save-state and
  NVRAM files, session-specific restore paths, and shutdown isolation
- Phase 1B runtime isolation is complete on the reference host
- Phase 1A is accepted for orchestration with objective timing and the
  rendered-player resource soak explicitly deferred
- a runnable Phase 1C control-plane service exposes health, readiness, and a
  versioned `/api/v1` boundary
- shared control-plane types define canonical lifecycle states and runtime-host
  capability, registration, heartbeat, and status payloads
- the control plane accepts idempotent host registration and heartbeats and
  rejects stale or conflicting heartbeat sequences
- the runtime-host agent discovers CPU, memory, supported H.264 encoders, and
  the MAME adapter, then registers and sends recurring sequenced heartbeats
- a 15-assertion live Debian test validated registration, capability reporting,
  heartbeat progression, graceful shutdown, and listener cleanup
- SQLite preserves runtime-host identity and heartbeat sequence across a real
  control-plane restart
- heartbeat expiry marks a host offline, and agent restart resumes its sequence
  and returns the host online
- the runtime agent verifies the four-title legal allowlist with MAME and
  publishes automatically discovered native runtime profiles
- the SQLite catalog exposes Aliens, Killer Instinct, TMNT, and WWF
  WrestleMania with per-host online/offline availability
- SQLite transactionally allocates sessions to online compatible hosts and
  reserves non-overlapping media/input ports
- time-limited connection grants carry the assigned host and ports; their
  128-bit token is enforced by the session-specific UDP input endpoint
- runtime hosts poll versioned assignments, launch the configured
  `session-runtime`, and report requested through active lifecycle transitions
- the agent supervises normal stops and unexpected MAME exits without stale
  PIDs or orphaned runtime children
- host heartbeat expiry moves its nonterminal sessions to `runtime_lost`
- the seat client browses the catalog, requests a game, launches FFplay,
  sends authenticated controls, stops the session, and returns to browsing
- a 32-assertion isolated real-MAME smoke test proves authenticated control,
  normal termination,
  injected runtime loss, recovery to browsing, and the complete seat workflow
- Phase 1C control-plane orchestration is complete on the reference host

## In progress

- Phase 2 shared-session discovery and player-slot modeling
- session responses expose explicit player slots derived from each game's
  maximum player count
- the seat client lists active sessions and occupied/open slots while browsing
- the seat client discovers active sessions through a seat-safe summary API
  that omits per-player connection grants and reports preview availability
- the control plane supports atomic, lease-based reservation, connection,
  disconnection, reconnection, and release of player slots in ready/active
  sessions
- joined seats route gameplay input to their assigned player number
- same-PC development joins default to input-only to avoid local media-port
  binding conflicts; `--joined-media` exists for separate display/host
  experiments
- the development client exposes media-only spectator selection for active
  sessions without claiming a player slot
- the control plane can allocate distinct spectator media-port grants for
  active sessions and exposes those ports to the runtime host assignment feed;
- session-runtime can launch FFmpeg with primary plus spectator UDP outputs
  when those grants are present before the process starts
- running sessions can pick up newly granted spectator media ports through the
  runtime host's refreshed spectator port file and session-runtime's Rust
  MPEG-TS fan-out loop
- control-plane media and input UDP port pools are configurable for n-seat
  development and deployment; session allocation also avoids active spectator
  media ports when launching new games
- game catalog entries now carry editable presentation metadata for an admin
  module, including descriptions, artwork/marquee/screenshot/logo paths,
  manufacturer/year/genre, control notes, and player-count overrides

## Not started

- low-cost active-session previews
- lower-bitrate preview streams distinct from full-quality spectator media

## Next milestone

Design and implement low-cost active-session previews and spectator/media
fan-out so browsing and joined seats can observe sessions without requiring one
full-quality gameplay encoder or one shared local UDP media port per viewer.
Richer fixed-position and character-aware player-slot metadata follows that
media path.
