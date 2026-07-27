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

- `available`
- `reserved`
- `connected`
- `reconnecting`
- `released`

A reservation includes a lease expiration and seat identity. Reservation and release operations must be atomic and idempotent.

## Fixed and interchangeable slots

Packages declare one of:

- `generic` — player numbers are functionally interchangeable
- `positioned` — slot maps to a cabinet side or team
- `fixed-character` — slot selects a specific character
- `custom` — package provides explicit labels and behavior

The UI should expose meaningful labels rather than assuming every game is simply Player 1 through Player 4.
