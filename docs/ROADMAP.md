# 4-Play Roadmap

## Two different milestones

The technical feasibility milestone and the product MVP are intentionally separate.

- **Technical feasibility** proves that centrally hosted emulation can feel responsive, remain isolated, and recover predictably.
- **Product MVP** proves the experience that makes 4-Play distinctive: multiple seats can discover, preview, start, join, and spectate sessions without operator intervention.

A successful streaming experiment is necessary, but it is not the product MVP.

## Phase 0 — Foundation

- capture product vision and requirements
- establish architecture and ADR process
- define legal game-package boundaries
- select the Phase 1 technical experiments
- create repository contribution and issue workflow
- inventory the proposed Linux runtime host

**Exit:** Phase 1A can begin without unresolved product-boundary or test-environment questions.

## Phase 1A — Remote-Play Feasibility

Answer one question: can one thin seat client play one centrally hosted MAME game over the wired reference network with acceptable controls, video, and audio?

- one hardcoded legal test game
- one seat and one runtime host
- manually configured addresses
- direct controller transport experiment
- low-latency video and audio experiment
- repeatable latency and jitter measurement
- local-emulation baseline
- provisional reconnect behavior
- no catalog database, scheduler, or player-slot service

**Exit:** one remote seat is playable, measurements are documented, and there is enough evidence to accept, revise, or reject ADR-0002.

## Phase 1B — Runtime Isolation

Prove that concurrent emulator sessions can coexist safely.

- two MAME instances
- two session-specific virtual controllers
- separate working, save, and temporary directories
- no cross-session input leakage
- independent process supervision and termination
- stuck-button failsafe on disconnect
- orphan-process cleanup
- forced-crash recovery test

**Exit:** two sessions operate independently and one session can fail without affecting the other.

## Phase 1C — Control-Plane Orchestration

Replace manual experiment configuration with the first durable platform slice.

- control-plane service skeleton
- runtime-host registration and heartbeat
- minimal legal test catalog
- versioned session lifecycle protocol
- MAME runtime adapter
- session allocation and connection grants
- minimal seat client launch workflow
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
