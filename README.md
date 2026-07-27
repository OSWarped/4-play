# 4-Play

4-Play is a distributed arcade platform for social, cooperative, competitive, and independent retro gaming.

The flagship experience is a four-seat arcade table. Each seat can browse and launch a game independently, join an active multiplayer session in a selected player position, or spectate gameplay already happening elsewhere in the arcade.

The table is the reference implementation. The reusable product is the platform that coordinates game discovery, emulator runtimes, player slots, input routing, live session previews, streaming, health, and recovery.

## Status

4-Play is in **Phase 0: foundation and architecture**.

The first implementation milestone is intentionally narrow: prove that one thin seat client can control one centrally hosted MAME session with acceptable latency and without input leaking into another concurrent session.

## Product principles

- Social discovery matters more than presenting a giant ROM list.
- Seats should behave like terminals, while latency-sensitive media and input travel directly to the runtime host when practical.
- The control plane and real-time data plane are separate concerns.
- Game-specific player positions and cabinet behavior must be preserved.
- The system should fail visibly, recover predictably, and remain playable at the end of every major phase.
- ROMs, BIOS files, and copyrighted game media do not belong in this repository.

## Repository map

- `docs/requirements/` — product and quality requirements
- `docs/architecture/` — system boundaries, components, and data flows
- `docs/design/` — detailed design for game packages, player slots, and input
- `docs/adr/` — accepted and proposed architectural decisions
- `server/` — control-plane API and orchestration
- `runtime/` — emulator-host agent and runtime adapters
- `clients/` — seat, operator, and spectator clients
- `shared/` — shared schemas, protocol definitions, and domain types
- `tools/` — development, validation, packaging, and administration tools
- `examples/` — legal sample manifests and synthetic fixtures
- `assets/` — project-owned branding and documentation assets

## Phase 1 success criteria

Phase 1 is complete when:

1. A seat can browse a small legal test catalog.
2. A seat can request a MAME session.
3. The runtime host launches and monitors the emulator process.
4. The seat sends controller state to a session-specific virtual controller.
5. The seat receives video and audio from the runtime host.
6. Added button-to-photon latency is measured and documented.
7. Two concurrent sessions run without cross-session input leakage.
8. A crashed runtime is detected and reported to the seat.
9. The full proof of concept can be started from documented commands.

See [the roadmap](docs/ROADMAP.md), [requirements](docs/requirements/REQUIREMENTS.md), and [architecture](docs/architecture/ARCHITECTURE.md).
