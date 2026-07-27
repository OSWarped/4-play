# 4-Play

4-Play is a distributed arcade platform for social, cooperative, competitive, and independent retro gaming.

The flagship experience is a four-seat arcade table. Each seat can browse and launch a game independently, join an active multiplayer session in a selected player position, or spectate gameplay already happening elsewhere in the arcade.

The table is the reference implementation. The reusable product is the platform that coordinates game discovery, emulator runtimes, player slots, input routing, live session previews, streaming, health, and recovery.

## Status

4-Play is in **Phase 0: foundation and architecture**.

The project deliberately separates two milestones:

- **Technical feasibility:** prove that a thin seat client can control centrally hosted MAME with practical latency, strict session isolation, and predictable recovery.
- **Product MVP:** prove the social experience by allowing multiple seats to discover, preview, start, join, select positions in, and spectate active sessions.

A successful remote-streaming experiment is necessary, but it is not by itself the 4-Play product MVP.

## Product principles

- Social discovery matters more than presenting a giant ROM list.
- Seats are stateless with respect to authoritative emulator and game state, but may render UI, decode media, read controls, cache assets, and recover connections locally.
- The proposed control-plane and real-time data-plane separation must be validated experimentally.
- Game-specific player positions and cabinet behavior must be preserved.
- The system should fail visibly, recover predictably, and remain playable at the end of every major phase.
- ROMs, BIOS files, and copyrighted game media do not belong in this repository.

## Repository map

- `docs/requirements/` — product and quality requirements
- `docs/architecture/` — system boundaries, components, and data flows
- `docs/design/` — detailed design for game packages, player slots, input, and previews
- `docs/adr/` — accepted and proposed architectural decisions
- `server/` — control-plane API and orchestration
- `runtime/` — emulator-host agent and runtime adapters
- `clients/` — seat, operator, and spectator clients
- `shared/` — shared schemas, protocol definitions, and domain types
- `tools/` — development, validation, packaging, and administration tools
- `examples/` — legal sample manifests and synthetic fixtures
- `assets/` — project-owned branding and documentation assets

## First technical experiments

### Phase 1A — Remote-play feasibility

One hardcoded legal test game, one runtime host, one separate seat, manually configured addresses, synchronized controls/video/audio, and repeatable latency measurement.

### Phase 1B — Runtime isolation

Two independent MAME sessions, separate virtual controllers and working directories, no input leakage, and failure cleanup that does not interrupt the surviving session.

### Phase 1C — Control-plane orchestration

A minimal catalog, runtime registration, session allocation, connection grants, lifecycle state, and recovery to browsing.

See [the roadmap](docs/ROADMAP.md), [requirements](docs/requirements/REQUIREMENTS.md), and [architecture](docs/architecture/ARCHITECTURE.md).
