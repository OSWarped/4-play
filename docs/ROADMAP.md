# 4-Play Roadmap

## Phase 0 — Foundation

- capture product vision and requirements
- establish architecture and ADR process
- define legal game-package boundaries
- select the Phase 1 technical spike
- create repository contribution and issue workflow

**Exit:** the first vertical slice is implementable without unresolved product-boundary questions.

## Phase 1 — Single Remote Seat

- control-plane service skeleton
- runtime-host registration and heartbeat
- small legal test catalog
- MAME runtime adapter
- session-specific virtual controller
- remote controller transport
- low-latency video and audio
- latency measurement harness
- second concurrent session and isolation test
- crash detection and cleanup

**Exit:** one remote seat feels acceptably close to local emulation on a wired LAN, with documented latency and failure results.

## Phase 2 — Shared Sessions

- active-session discovery
- low-cost live previews
- explicit player-slot model
- join and reconnect leases
- spectator mode
- fixed-character and positioned cabinet profiles

**Exit:** a second seat can discover, inspect, and join an active compatible session without operator intervention.

## Phase 3 — Four-Seat Table

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
