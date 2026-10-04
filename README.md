# 4-Play

4-Play is a distributed arcade platform for social, cooperative, competitive, and independent retro gaming.

The flagship experience is a four-seat arcade table. Each seat can browse and launch a game independently, join an active multiplayer session in a selected player position, or spectate gameplay already happening elsewhere in the arcade.

The table is the reference implementation. The reusable product is the platform that coordinates game discovery, emulator runtimes, player slots, input routing, live session previews, streaming, health, and recovery.

## Current status

4-Play has completed its reference-host remote-play and runtime-isolation proof
and has begun Phase 1C control-plane orchestration.

Validated capabilities include:

- one-command launch of a headless MAME session from the Rust session runtime
- per-session working, configuration, NVRAM, state, snapshot, and diff directories
- raw video and raw PCM audio emitted directly by the custom MAME build
- concurrent Rust readers for MAME video and audio FIFOs
- bounded media queues that prefer dropping stale video over accumulating latency
- one FFmpeg encoder process per session
- live H.264/AAC delivery over UDP MPEG-TS to a remote Windows seat
- synchronized audio and video for Aliens, TMNT, and Killer Instinct
- correct handling of game-specific resolution and refresh rate
- simultaneous independent MAME sessions
- CHD-backed game launch and streaming
- automatic MAME and encoder lifecycle ownership by the runtime
- clean Ctrl+C termination without orphaned MAME or FFmpeg processes
- creation of a Linux virtual game controller through `/dev/uinput`
- verified virtual axes and buttons through the Linux joystick subsystem

Not yet complete:

- automatic MAME metadata discovery
- durable session state transitions and production-grade cleanup
- runtime-host agent registration and recurring heartbeat
- catalog, allocation, connection grants, and the complete seat launch workflow

The technical-feasibility milestone remains distinct from the product MVP:

- **Technical feasibility:** prove that a thin seat can control centrally hosted MAME with practical latency, strict session isolation, and predictable recovery.
- **Product MVP:** prove the social experience by allowing multiple seats to discover, preview, start, join, select positions in, and spectate active sessions.

A working remote media stream is necessary, but it is not by itself the 4-Play product MVP.

## Current runtime path

```text
Headless MAME
  ├─ raw video FIFO
  └─ raw PCM audio FIFO
          ↓
Rust session runtime
  ├─ concurrent media readers
  ├─ bounded low-latency queues
  ├─ session metrics
  ├─ MAME process ownership
  └─ FFmpeg process ownership
          ↓
FFmpeg
  ├─ H.264 video
  ├─ AAC audio
  └─ MPEG-TS over UDP
          ↓
Remote seat player
```

The validated input path is:

```text
Windows seat-input client
  → state-oriented UDP input
  → Rust session runtime
  → session-owned /dev/uinput controller
  → MAME
```

The next implementation milestone is durable SQLite-backed host state and
offline detection when heartbeats expire.

## Product principles

- Social discovery matters more than presenting a giant ROM list.
- Seats are stateless with respect to authoritative emulator and game state, but may render UI, decode media, read controls, cache assets, and recover connections locally.
- The control plane and real-time data plane remain separate concerns.
- Game-specific player positions and cabinet behavior must be preserved.
- The system should fail visibly, recover predictably, and remain playable at the end of every major phase.
- ROMs, BIOS files, CHDs, saves, and copyrighted game media do not belong in this repository.

## Repository map

- `runtime/session-runtime/` — Rust session runtime, MAME lifecycle, media bridge, and encoder integration
- `runtime/host-agent/` — runtime capability discovery, registration, and heartbeat client
- `control-plane/server/` — Phase 1C HTTP control-plane service
- `clients/seat-input/` — Windows development client for state-based keyboard input
- `shared/control-protocol/` — versioned control-plane messages and lifecycle states
- `shared/input-protocol/` — versioned seat-to-runtime controller-state packets
- `tools/uinput-test/` — development validation for runtime-created Linux virtual controllers
- `docs/requirements/` — product and quality requirements
- `docs/architecture/` — system boundaries, components, and validated data flows
- `docs/experiments/` — experiment records and evidence
- `docs/adr/` — accepted and proposed architectural decisions
- `clients/` — future seat, operator, and spectator clients
- `shared/` — shared schemas, protocol definitions, and domain types
- `tools/` — development, validation, packaging, and administration tools
- `examples/` — legal sample manifests and synthetic fixtures
- `assets/` — project-owned branding and documentation assets

## Development phases

### Phase 1A — Remote-play feasibility

Media transport, runtime-owned controllers, and direct remote seat input are
validated. The architecture is accepted for Phase 1C; objective timing and the
rendered-player resource soak are deferred rather than blocking.

### Phase 1B — Runtime isolation

Complete on the reference host. Media, controller, process, abnormal
child-failure, save-state, and NVRAM isolation have all been demonstrated with
two concurrent sessions.

### Phase 1C — Control-plane orchestration

A minimal catalog, runtime registration, session allocation, connection grants, lifecycle state, and recovery to browsing.

See [the roadmap](docs/ROADMAP.md), [requirements](docs/requirements/REQUIREMENTS.md), [architecture](docs/architecture/ARCHITECTURE.md), and [runtime validation record](docs/experiments/SESSION_RUNTIME_VALIDATION.md).
