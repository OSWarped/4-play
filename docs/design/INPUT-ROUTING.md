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

## Phase 1C transport

The orchestrated path uses UDP directly between `seat-input` and
`session-runtime`. Version 2 packets contain the session grant's 128-bit token,
player slot, monotonic sequence number, complete button bitset, signed X/Y
axes, and flags. The seat sends transitions immediately and a heartbeat every
50 ms. The runtime silently discards packets whose token does not match its
assigned session.

The runtime accepts one source per session, discards stale packets, and
neutralizes its virtual controller after a 250 ms timeout. Version 1
unauthenticated packets remain available only through explicit direct
diagnostic mode. Encryption and replay-resistant timestamps remain required
before this protocol can be used outside the trusted development LAN.

## Future n-seat port allocation

Future multi-seat deployments should treat media and input ports as runtime-host
resources allocated from configured pools, not as hard-coded seat identities.
The control plane and runtime host should be the source of truth for the exact
ports assigned to each active session, and the seat should use only the
endpoints returned in its session grant.

Recommended model:

- reserve bounded UDP ranges per runtime host
- allocate one media port and one input port per active session
- return the assigned `host:media_port`, `host:input_port`, and session input
  token in the grant
- release ports when the session ends
- reclaim ports from expired or orphaned sessions after a timeout
- open the reserved ranges in the runtime-host firewall from trusted seat
  networks instead of adding one firewall rule per seat

Example default ranges:

```text
control plane TCP: 8080
media UDP:         41000-41999
input UDP:         42000-42999
```

An example allocation might assign:

```text
session A: media 41000, input 42000
session B: media 41001, input 42001
session C: media 41002, input 42002
```

Those numeric assignments are an implementation detail. The seat client must
not infer ports from its seat number; it must follow the session grant. This
keeps the model compatible with reconnects, multiple runtime hosts, uneven
session lifetimes, future join/spectator flows, and cabinet configurations
where a physical seat may not map one-to-one with a process.

The ranges should become configurable before production deployments, for
example:

```text
FOURPLAY_MEDIA_PORT_START=41000
FOURPLAY_MEDIA_PORT_COUNT=1000
FOURPLAY_INPUT_PORT_START=42000
FOURPLAY_INPUT_PORT_COUNT=1000
```

Installation documentation should derive firewall requirements from those
configured ranges.
