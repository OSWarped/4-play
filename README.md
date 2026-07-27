# 4-Play

4-Play is a distributed arcade platform designed for social, cooperative,
competitive, and independent retro gaming.

The platform allows arcade seats and other supported clients to browse games,
start centrally hosted emulator sessions, join active sessions, select
available player positions, spectate gameplay, and receive low-latency video
and audio while sending controller input to the emulator host.

The flagship reference implementation is a four-seat arcade table with four
independent displays and control stations. Each seat may run independently or
participate in a shared multiplayer session.

## Project Status

4-Play is currently in the requirements and Phase 1 architecture stage.

Phase 1 will prove that a seat client can:

1. Browse a small game catalog.
2. Request a game session.
3. Send controller input to a centrally hosted emulator.
4. Receive low-latency video and audio.
5. Provide a gameplay experience close to locally running emulation.

## Repository Structure

- `docs/` — product requirements, architecture, design, and decisions
- `server/` — central 4-Play server and emulator host agent
- `clients/` — seat and operator clients
- `shared/` — shared protocol and domain libraries
- `tools/` — development and administration utilities
- `examples/` — example game-package definitions without copyrighted ROMs
- `assets/` — 4-Play branding and documentation assets

## Core Design Principles

- Emulator-agnostic architecture
- Session-specific virtual controllers
- Direct real-time communication between seats and emulator hosts
- Centralized session discovery and orchestration
- Player-selectable slots where controller position has game-specific meaning
- Rich game packages containing metadata, artwork, and preview media
- A playable system at the end of every major development phase