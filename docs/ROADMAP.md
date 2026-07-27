# 4-Play Roadmap

## Phase 0 — Foundation

- Establish repository structure
- Capture product vision
- Capture requirements
- Define Phase 1 architecture
- Record major architectural decisions
- Configure the Rust development environment

## Phase 1 — Single Remote Seat

- One central server
- One emulator host
- One seat client
- Small game catalog
- Start one MAME game
- Create session-specific virtual controllers
- Send controller state from seat to host
- Receive video and audio from host
- Measure added button-to-photon latency
- Run one additional concurrent session without input leakage

## Phase 2 — Shared Session Joining

- Display active sessions
- Display available player slots
- Reserve a selected player slot
- Join an active emulator session
- Route the joining seat to the correct virtual controller
- Support fixed-character and meaningful controller-position profiles
- Support a spectator client

## Phase 3 — Multiple Emulators

- Emulator-adapter interface
- Additional arcade and console emulators
- Runtime-specific controller mappings
- Runtime-specific launch and join behavior

## Phase 4 — Game Packages

- Rich metadata
- Marquees and splash artwork
- Screenshots
- Cabinet and control-panel imagery
- Gameplay-preview video
- Runtime variants
- Game-package validation and import tools

## Phase 5 — Four-Seat Reference Table

- Four independent displays
- Four control stations
- Independent and shared play
- Ergonomic physical prototype
- Kiosk startup and recovery
- Extended multiplayer and reliability testing