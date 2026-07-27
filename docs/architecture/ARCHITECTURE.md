# 4-Play Architecture

## Architectural thesis

4-Play uses a **central control plane** plus one or more **runtime hosts**.

Seats are thin clients from a product perspective: they do not own emulator state. The control plane coordinates sessions, while latency-sensitive controller input, video, and audio travel directly between a seat and its assigned runtime host whenever practical.

This preserves the terminal model without forcing every frame through the central API server.

## Major components

### Control-plane server

Responsibilities:

- device identity and registration
- game catalog and package metadata
- seat presence and capabilities
- session creation and lifecycle state
- player-slot reservation
- runtime-host selection
- authorization and short-lived connection grants
- active-session discovery
- operator APIs
- durable configuration and audit events

The control plane does not process normal controller packets or transcode every gameplay frame.

### Runtime host agent

Responsibilities:

- report CPU, GPU, encoder, controller, and storage capabilities
- validate and stage approved game packages
- create session-specific working directories
- create isolated virtual controllers
- launch and supervise emulator processes
- expose session media and input endpoints
- publish health and session events
- tear down processes and temporary resources

The first deployment may place the control plane and runtime host agent on one Linux machine. They remain separate logical components so additional Linux or Windows runtime hosts can be added later.

### Seat client

Responsibilities:

- attract mode
- catalog browsing
- active-session browsing
- live preview display
- player-slot selection
- game controls and operator-safe escape actions
- direct real-time connection to the assigned runtime host
- graceful handling of reconnect, host loss, and session termination

### Operator client

Responsibilities:

- view seat and host health
- inspect active sessions
- drain or disable a runtime host
- terminate an unhealthy session
- manage approved game packages
- review latency and reliability telemetry

## Control plane and data plane

### Control plane

Reliable request/response and event delivery handles device registration, catalog queries, session creation, slot reservations, lifecycle changes, operator actions, and connection negotiation. HTTP plus WebSocket or server-sent events is sufficient initially.

### Real-time data plane

Direct seat-to-runtime communication handles controller state, video, audio, and stream timing feedback. WebRTC is the leading candidate, but Phase 1 must measure practical local-network alternatives rather than standardizing prematurely.

## Session lifecycle

Canonical states:

1. `requested`
2. `allocating`
3. `starting`
4. `ready`
5. `active`
6. `stopping`
7. `stopped`

Failure states include `allocation_failed`, `launch_failed`, `runtime_lost`, `unhealthy`, and `terminated`.

State transitions are owned by the control plane. Runtime facts originate from the runtime host. Commands must be idempotent.

## Player slots

A session exposes logical player slots defined by its game package. A slot may represent a generic player number, fixed cabinet position, fixed character, team side, or spectator access.

Slot reservations are leases. A disconnected seat receives a reconnect grace period before its slot becomes available.

## Live previews

Active sessions publish a low-cost preview stream or periodic preview frames. Preview availability must never block gameplay or require one full-quality encoder per browsing seat.

Evaluation order:

1. reuse one encoded session stream with low-rate subscribers
2. provide a low-bitrate simulcast layer
3. produce periodic JPEG or WebP frames
4. fall back to package artwork

## Deployment stages

### Phase 1

One Linux machine may run the control-plane server, database, runtime host agent, MAME, and capture/encoding processes. A seat client runs on separate hardware.

### Later

- multiple runtime hosts
- optional Windows runtime hosts
- dedicated control-plane server
- operator stations
- many seats and table types

## Persistence

Use a relational database for devices, game packages, runtime hosts, sessions, lifecycle events, player-slot reservations, configuration, and audit records. High-frequency controller and media packets are not persisted.

SQLite is acceptable for the first single-server proof of concept. PostgreSQL is preferred before multi-host or commercial deployment.

## Observability

Every session receives a correlation ID. Logs and metrics should include session ID, seat ID, runtime host ID, game package version, emulator adapter version, launch duration, stream setup duration, input packet loss, measurable encode/decode latency, disconnects, and recovery events.

## Failure model

The design assumes processes, seats, networks, and hosts can fail independently.

Required behaviors:

- runtime heartbeat loss changes session health
- orphan emulator processes are reaped
- duplicate start/stop commands are safe
- stale slot leases expire
- a seat can return to browsing without rebooting
- one failed session does not terminate unrelated sessions
