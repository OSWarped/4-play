# 4-Play Requirements

## Terminology

- **Seat** — a display, controls, audio endpoint, and seat-client instance.
- **Session** — one running emulator instance and its assigned resources.
- **Runtime host** — a machine capable of launching and supervising sessions.
- **Player slot** — a game-defined controller position available to a seat.
- **Game package** — approved metadata and typed launch configuration for a title.

## Functional requirements

### Discovery and attract mode

- A seat shall enter attract mode after a configurable idle period.
- Input at the seat shall exit attract mode.
- A seat shall show games available to start and active sessions available to join or spectate.
- Active-session cards shall show game, health, occupied slots, open slots, and preview availability.

### Starting sessions

- A seat shall request a session without knowing emulator command-line details.
- The control plane shall select a compatible healthy runtime host.
- The runtime host shall create session-specific storage and controller resources.
- The runtime host shall report launch success or a diagnosable failure.
- The seat shall receive connection information only for its assigned session.

### Joining and player slots

- A seat shall inspect available player slots before joining.
- A seat shall be able to request a specific compatible slot.
- Slot assignment shall be atomic and lease-based.
- The game package shall define whether slot position has gameplay meaning.
- A disconnected seat shall receive a configurable reconnect grace period.

### Input

- Input from one seat shall reach only the assigned session and player slot.
- Input shall never leak between sessions.
- The runtime host shall expose only package-approved control mappings.
- Digital buttons and analog axes shall be supported.
- Operator escape actions shall be distinct from game input.

### Media

- A playing seat shall receive synchronized video and audio.
- A browsing seat may receive a lower-cost active-session preview.
- Preview generation shall not materially degrade active gameplay.

### Operations

- Operators shall view registered seats, runtime hosts, and sessions.
- Operators shall terminate sessions and drain runtime hosts.
- The system shall identify unhealthy and orphaned sessions.
- Runtime processes shall be cleaned up after normal or abnormal termination.

## Non-functional requirements

### Latency

- Phase 1 shall measure end-to-end button-to-photon latency.
- Local-emulation baseline and added remote latency shall be recorded separately.
- The initial target is no more than 50 ms of added median latency on a healthy wired LAN.
- The stretch target is no more than 30 ms of added median latency.
- Percentile results and test methodology shall be documented.

### Isolation and recovery

- Concurrent sessions shall not share controller state, saves, processes, or temporary files.
- A failed session shall not stop unrelated sessions.
- Runtime-host heartbeat loss shall be detected.
- Seats shall return to browsing after session loss without rebooting.
- Start, stop, and cleanup commands shall be idempotent.

### Security and maintainability

- Devices shall authenticate to the control plane.
- Runtime grants shall be scoped and short-lived.
- Clients shall not submit arbitrary executable paths or command-line arguments.
- Emulator-specific behavior shall be isolated behind adapters.
- Public protocol messages shall be versioned.
- Secrets, ROMs, BIOS files, saves, and copyrighted media shall not be committed.

## Phase 1 acceptance test

The Phase 1 demonstration shall:

1. start from a clean documented environment
2. connect one seat and one runtime host
3. browse a legal test catalog
4. launch one MAME title
5. play using the remote seat
6. capture latency measurements
7. launch a second independent session
8. prove input isolation
9. force-stop one emulator process
10. show correct detection, cleanup, and seat recovery
