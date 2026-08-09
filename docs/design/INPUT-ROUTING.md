# Input Routing Design

## Goals

- low latency
- strict session and slot isolation
- predictable reconnect behavior
- emulator-independent seat protocol
- package-controlled mappings

## Input path

```text
physical controls
  -> seat input adapter
  -> normalized controller state
  -> authenticated session channel
  -> runtime host input router
  -> session-specific virtual controller
  -> emulator
```

The control-plane server authorizes the route but does not relay normal controller traffic.

## Controller state

Transmit current state plus a monotonically increasing sequence number:

- buttons as a bitset
- signed analog axes
- optional triggers
- client timestamp
- sequence number

Send state at a fixed rate and immediately on important transitions when practical. The runtime host discards stale or unauthorized packets.

## Isolation

Every accepted packet must resolve through authenticated seat identity, an active session grant, the assigned player slot, and a runtime-local virtual controller handle.

No global keyboard injection is permitted for normal gameplay.

## Disconnect behavior

On timeout, the runtime host shall release all pressed buttons and center analog axes. A stuck-button failsafe is mandatory.

During the reconnect grace period, the slot remains assigned but input remains neutral until the seat re-authenticates.

## Phase 1A development transport

The first state-based experiment uses UDP directly between `seat-input` and
`session-runtime`. Packets contain a protocol version, player slot, monotonic
sequence number, complete button bitset, signed X/Y axes, and development stop
flag. The seat sends transitions immediately and a heartbeat every 50 ms.

The runtime accepts one source per session, discards stale packets, and
neutralizes its virtual controller after a 250 ms timeout. Authentication,
session grants, timestamps, and encryption remain required before this protocol
can be used outside the trusted development LAN.
