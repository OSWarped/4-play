# Foundation Summary

This foundation establishes the product and engineering boundaries for the first playable 4-Play slice.

## Decisions now captured

- 4-Play begins as a monorepository.
- Seats are thin clients and do not own emulator state.
- The control plane coordinates identity, catalog, sessions, player slots, and host selection.
- Controller input, video, and audio travel directly between seats and assigned runtime hosts when practical.
- Phase 1 may run the control plane and runtime host on the existing Linux server.
- Emulator behavior is isolated behind typed adapters and validated game packages.
- Player positions are modeled explicitly because cabinet position may affect character, team, or controls.

## First proof

The first implementation should prove one remote seat, one MAME session, direct input and media, measured latency, a second isolated session, and recovery from a forced emulator crash.
