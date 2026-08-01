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

Status: **in progress**.

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

Still required:

- connect the virtual controller to the launched MAME session
- implement seat-to-runtime controller transport
- neutralize controls on disconnect or timeout
- measure local and remote button-to-photon latency
- record median, 95th-percentile, and 99th-percentile results
- document host and client resource use

**Exit:** one remote seat can control a centrally hosted session, synchronized media remains stable, disconnect behavior is safe, and latency measurements support a go, revise, or pivot decision.

## Phase 1B — Runtime Isolation

Status: **partially validated ahead of schedule**.

Already demonstrated:

- two simultaneous MAME instances
- separate session directories and media FIFOs
- independent Rust media bridges and encoders
- different game resolutions and refresh rates in parallel
- no observed media cross-talk between Aliens and TMNT
- one-command ownership of MAME and FFmpeg per session

Still required:

- two session-specific virtual controllers
- proof of no cross-session input leakage
- concurrent save and NVRAM validation
- abnormal process termination and recovery tests
- cleanup of stale processes and resources
- proof that one failed session does not interrupt another

**Exit:** two complete playable sessions operate independently and one can stop without affecting the other.

## Phase 1C — Control-Plane Orchestration

Status: **not started**.

- control-plane service skeleton
- runtime-host registration and heartbeat
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
