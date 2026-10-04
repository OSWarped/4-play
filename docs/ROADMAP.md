# 4-Play Roadmap

## Milestones

Technical feasibility and the product MVP remain separate milestones.

- **Technical feasibility** proves that centrally hosted emulation can feel responsive, remain isolated, and recover predictably.
- **Product MVP** proves the social experience: multiple seats can discover, preview, start, join, and spectate sessions without operator intervention.

## Phase 0 — Foundation

Status: **complete enough for active implementation**.

Completed:

- product vision and requirements
- architecture and ADR process
- legal game-package boundaries
- repository and issue workflow
- X-free Linux MAME validation
- Rust workspace and initial session runtime

## Phase 1A — Remote-Play Feasibility

Status: **accepted for orchestration; optional measurements deferred**.

Validated:

- one-command headless MAME launch from Rust
- direct raw video and PCM audio output from MAME
- synchronized H.264/AAC streaming over the wired LAN
- UDP MPEG-TS playback on a Windows seat
- bounded media queues that avoid unbounded latency growth
- game-specific resolution and refresh-rate handling
- synchronized playback for Aliens, TMNT, and Killer Instinct
- CHD-backed title support
- a Linux virtual controller with two axes and eight buttons
- runtime-owned virtual-controller input consumed by MAME
- direct Windows-seat-to-runtime controller-state transport
- simultaneous directions and buttons
- sequence rejection, timeout neutralization, and reconnect behavior
- subjectively responsive control-to-video play through low-buffer FFplay
- synchronized action and game sound during live TMNT play after applying
  wall-clock timestamps to the independent raw media inputs
- thirty-minute sequential soak across TMNT, Aliens, and Killer Instinct with
  clean cleanup, stable resources, and no audio drops
- single-session host resource measurements and a Windows decoder-only lower
  bound
- native-rate output for the exact-60-Hz TMNT profile, validated at 60.06 fps,
  with Aliens and Killer Instinct retaining their non-integer native rates

Deferred, non-blocking measurements:

- measure local and remote button-to-photon latency
- record median, 95th-percentile, and 99th-percentile results
- measure a rendered Windows player's resource use and memory plateau

**Exit achieved:** one remote seat controls a centrally hosted session,
synchronized media remains stable, disconnect behavior is safe, and repeated
live play supports proceeding with the architecture. The project owner waived
objective timing as a Phase 1C prerequisite.

## Phase 1B — Runtime Isolation

Status: **validated on the reference host**.

Already demonstrated:

- two simultaneous MAME instances
- separate session directories and media FIFOs
- independent Rust media bridges and encoders
- different game resolutions and refresh rates in parallel
- no observed media cross-talk between Aliens and TMNT
- one-command ownership of MAME and FFmpeg per session
- two simultaneously running sessions own distinct virtual input devices and
  receive different controller patterns without event-device cross-talk
- orderly shutdown of one session removes its processes and virtual controller
  while the other session continues streaming and accepting input
- manual TMNT and Killer Instinct play confirmed that each Windows input client
  controls only its assigned MAME session
- unexpected MAME and FFmpeg termination in one session produces a failed
  runtime result and cleans its remaining process, controller, input port, and
  transient media FIFOs
- the unaffected session continues streaming and accepting controller input
  through both child-failure cases
- a failed session can restart repeatedly with the same session ID and ports
  while preserving its configuration and NVRAM directories
- graceful runtime shutdown asks MAME to exit normally so autosave state and
  NVRAM are flushed before the remaining process tree is reaped
- two concurrent WWF WrestleMania sessions write separate autosave and NVRAM
  files, restore only their assigned paths, and retain isolation when either
  session stops

**Exit achieved:** two complete playable sessions operate independently and one
can stop without affecting the other. See
[the persistence isolation record](testing/TWO-SESSION-PERSISTENCE-ISOLATION.md).

## Phase 1C — Control-Plane Orchestration

Status: **started**.

Implemented foundation:

- runnable Rust control-plane HTTP service
- health and readiness endpoints
- `/api/v1` service boundary
- shared, serialized canonical session states
- in-memory runtime-host registration, lookup, listing, and heartbeat API
- runtime-host agent with capability discovery, sequenced heartbeats, retry,
  and clean shutdown

Still required:

- durable control-plane storage
- heartbeat expiry and offline-host detection
- minimal legal test catalog
- versioned session lifecycle protocol
- MAME runtime adapter configuration
- automatic MAME metadata discovery
- session allocation and connection grants
- minimal seat launch workflow
- diagnosable failure and recovery states

**Exit:** a seat can browse the test catalog, request a session, connect to the assigned runtime, play, and return to browsing after normal termination or runtime loss.

## Phase 2 — Product MVP: Shared Sessions

- active-session discovery
- low-cost live previews
- explicit player-slot model
- atomic slot reservation and reconnect leases
- join an active compatible session
- spectator mode
- fixed-character and positioned cabinet profiles
- preview degradation that never blocks gameplay

**Exit:** multiple seats can discover what is happening, inspect available positions, start or join a game, and spectate without operator intervention.

## Phase 3 — Four-Seat Reference Table

- four independent seat clients
- independent and shared play
- kiosk startup and recovery
- physical controls and audio isolation
- table ergonomics and service access
- soak and abuse testing

**Exit:** the reference table operates for an extended session with predictable recovery.

## Phase 4 — Multiple Runtime Hosts and Emulators

- host capability scheduling
- Windows runtime-host support where justified
- emulator-adapter interface stabilization
- additional emulator adapters
- package validation and import tools
- storage and save policies

## Phase 5 — Arcade Operations

- operator dashboard
- fleet health and remote maintenance
- role-based administration
- package rollout and rollback
- telemetry retention
- commercial deployment hardening
