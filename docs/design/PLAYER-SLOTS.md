# Player Slot Design

## Purpose

A player slot is not merely an array index. In arcade games, controller position may determine character, team, side, cabinet position, or available controls.

## Slot definition

Each game package declares slots with:

- stable slot ID
- display name
- controller index expected by the emulator
- joinable flag
- spectator flag
- optional character or team label
- optional control-profile override
- optional artwork

Example:

```yaml
slots:
  - id: p1-cyclops
    label: Player 1 — Cyclops
    emulator_controller: 1
    joinable: true
  - id: p2-colossus
    label: Player 2 — Colossus
    emulator_controller: 2
    joinable: true
```

## Reservation lifecycle

Slot states:

- `open`
- `reserved`
- `occupied`
- `disconnected`

A reservation includes a lease expiration and seat identity. Reservation,
connect, disconnect, and release operations must be atomic and idempotent. The
initial Phase 2 implementation stores player slots with each session, exposes
them in session responses, allows seats to reserve open slots in ready or active
sessions, and lets a reserved seat become a fully playable joined seat through
its assigned player number.

An intentional seat exit releases the slot. If a joined seat is interrupted by
an external session/media end, the seat marks the slot `disconnected` and keeps
the seat identity for the reconnect grace period before the slot can become
open again.

## Fixed and interchangeable slots

Packages declare one of:

- `generic` — player numbers are functionally interchangeable
- `positioned` — slot maps to a cabinet side or team
- `fixed-character` — slot selects a specific character
- `custom` — package provides explicit labels and behavior

The UI should expose meaningful labels rather than assuming every game is simply Player 1 through Player 4.

## Current implementation

Session player slots now carry presentation metadata in addition to reservation
state:

- `label`
- optional `position`
- optional `character`
- optional `artwork_path`

New sessions receive safe generic defaults such as `Player 1` with position
`P1`. The seat browser displays these labels when listing active sessions.
The fields are part of the shared control protocol and default during
deserialization, so older persisted slots remain readable. A later package or
admin layer can replace the generic labels with cabinet-position or
fixed-character values without changing the slot lifecycle APIs.
