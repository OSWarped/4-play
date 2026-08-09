# 4-Play

4-Play is a distributed arcade platform for social, cooperative, competitive, and independent retro gaming.

The flagship experience is a four-seat arcade table. Each seat can browse and launch a game independently, join an active multiplayer session in a selected player position, or spectate gameplay already happening elsewhere in the arcade.

The table is the reference implementation. The reusable product is the platform that coordinates game discovery, emulator runtimes, player slots, input routing, live session previews, streaming, health, and recovery.

## Current status

4-Play has completed the first major runtime and media feasibility work and is now finishing the remote-input portion of Phase 1A.

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

- connecting the runtime-owned virtual controller to MAME
- seat-to-runtime network input
- disconnect neutralization and reconnect behavior
- objective local and remote button-to-photon latency measurements
- automatic MAME metadata discovery
- complete session state transitions and production-grade cleanup

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

The virtual-controller validation path currently exists separately:

```text
Rust uinput test
  → /dev/uinput
  → Linux input subsystem
  → virtual joystick with two axes and eight buttons
```

The next implementation milestone is to integrate that controller into `session-runtime`, prove that MAME consumes it, and then add the real seat input transport.

## Product principles

- Social discovery matters more than presenting a giant ROM list.
- Seats are stateless with respect to authoritative emulator and game state, but may render UI, decode media, read controls, cache assets, and recover connections locally.
- The control plane and real-time data plane remain separate concerns.
- Game-specific player positions and cabinet behavior must be preserved.
- The system should fail visibly, recover predictably, and remain playable at the end of every major phase.
- ROMs, BIOS files, CHDs, saves, and copyrighted game media do not belong in this repository.

## Repository map

- `runtime/session-runtime/` — Rust session runtime, MAME lifecycle, media bridge, and encoder integration
- `tools/uinput-test/` — development validation for runtime-created Linux virtual controllers
- `docs/requirements/` — product and quality requirements
- `docs/architecture/` — system boundaries, components, and validated data flows
- `docs/experiments/` — experiment records and evidence
- `docs/adr/` — accepted and proposed architectural decisions
- `server/` — future control-plane API and orchestration
- `clients/` — future seat, operator, and spectator clients
- `shared/` — future shared schemas, protocol definitions, and domain types
- `tools/` — development, validation, packaging, and administration tools
- `examples/` — legal sample manifests and synthetic fixtures
- `assets/` — project-owned branding and documentation assets

## Development phases

### Phase 1A — Remote-play feasibility

Current phase. Media transport and virtual-controller creation are validated. Remote seat input and objective latency measurement remain.

### Phase 1B — Runtime isolation

Media-side concurrent-session isolation has been demonstrated. Controller isolation, failure injection, and cleanup behavior still require testing.

### Phase 1C — Control-plane orchestration

A minimal catalog, runtime registration, session allocation, connection grants, lifecycle state, and recovery to browsing.

See [the roadmap](docs/ROADMAP.md), [requirements](docs/requirements/REQUIREMENTS.md), [architecture](docs/architecture/ARCHITECTURE.md), and [runtime validation record](docs/experiments/SESSION_RUNTIME_VALIDATION.md).
