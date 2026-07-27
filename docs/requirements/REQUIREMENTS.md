# 4-Play Requirements

## Terminology

- **Seat** — a display, controls, audio endpoint, and seat-client instance.
- **Thin seat client** — a client that does not own authoritative emulator or game state. It may render UI, decode media, play audio, read physical controls, cache presentation assets, and recover connections locally.
- **Session** — one running emulator instance and its assigned resources.
- **Runtime host** — a machine capable of launching and supervising sessions.
- **Player slot** — a game-defined controller position available to a seat.
- **Game package** — approved metadata and typed launch configuration for a title.
- **Technical feasibility milestone** — proof that remote centralized emulation can meet practical latency, isolation, and recovery goals.
- **Product MVP** — the first multi-seat experience that supports discovery, previews, starting, joining, slot selection, and spectating.

## Functional requirements

### Discovery and attract mode

- A seat shall enter attract mode after a configurable idle period.
- Input at the seat shall exit attract mode.
- A seat shall show games available to start and active sessions available to join or spectate.
- Active-session cards shall show game, health, occupied slots, open slots, and preview availability.

These are Product MVP requirements and are not required for the Phase 1A remote-play feasibility experiment.

### Starting sessions

- A seat shall request a session without knowing emulator command-line details.
- The control plane shall select a compatible healthy runtime host.
- The runtime host shall create session-specific storage and controller resources.
- The runtime host shall report launch success or a diagnosable failure.
- The seat shall receive connection information only for its assigned session.

These become required in Phase 1C. Phase 1A may use manually configured addresses and one hardcoded legal test title.

### Joining and player slots

- A seat shall inspect available player slots before joining.
- A seat shall be able to request a specific compatible slot.
- Slot assignment shall be atomic and lease-based.
- The game package shall define whether slot position has gameplay meaning.
- A disconnected seat shall receive a configurable reconnect grace period.

These are Product MVP requirements targeted for Phase 2.

### Input

- Input from one seat shall reach only the assigned session and player slot.
- Input shall never leak between sessions.
- The runtime host shall expose only package-approved control mappings.
- Digital buttons and analog axes shall be supported.
- Operator escape actions shall be distinct from game input.
- Input timeout shall release pressed buttons and center analog axes.

### Media

- A playing seat shall receive synchronized video and audio.
- A browsing seat may receive a lower-cost active-session preview.
- Preview generation shall not materially degrade active gameplay.
- Preview availability shall never block starting, joining, or continuing gameplay.

### Operations

- Operators shall view registered seats, runtime hosts, and sessions.
- Operators shall terminate sessions and drain runtime hosts.
- The system shall identify unhealthy and orphaned sessions.
- Runtime processes shall be cleaned up after normal or abnormal termination.

## Non-functional requirements

### Latency and consistency

Phase 1A shall measure end-to-end button-to-photon behavior rather than relying on perceived responsiveness alone.

The test report shall record:

- local-emulation baseline latency
- total remote button-to-photon latency
- added remote latency
- median, 95th-percentile, and 99th-percentile latency
- latency variation or jitter
- dropped, late, or out-of-order controller updates
- encode time where measurable
- network transit where measurable
- decode and presentation time where measurable
- test hardware, network path, game, resolution, frame rate, and transport configuration

The initial engineering goal on the wired reference network is:

- less than 30 ms added median latency
- less than 50 ms added 95th-percentile latency
- no recurring input-loss or stuck-input behavior

These values are provisional experiment goals, not final universal product requirements. Final acceptance targets shall be established after measurements across representative game categories, including cooperative beat-'em-ups and latency-sensitive fighting or action games.

### Isolation and recovery

- Concurrent sessions shall not share controller state, saves, processes, or temporary files.
- A failed session shall not stop unrelated sessions.
- Runtime-host heartbeat loss shall be detected once control-plane orchestration exists.
- Seats shall return to a usable state after session loss without rebooting.
- Start, stop, and cleanup commands shall be idempotent once those commands exist.

### Security and maintainability

- Devices shall authenticate to the control plane in Phase 1C and later.
- Runtime grants shall be scoped and short-lived when negotiated connections are implemented.
- Clients shall not submit arbitrary executable paths or command-line arguments.
- Emulator-specific behavior shall be isolated behind adapters.
- Public protocol messages shall be versioned.
- Secrets, ROMs, BIOS files, saves, and copyrighted media shall not be committed.

## Technical feasibility acceptance tests

### Phase 1A — Remote play

The demonstration shall:

1. start one approved MAME title on the proposed runtime host
2. accept controls from one separate thin seat client over the wired reference network
3. deliver synchronized video and audio to the seat
4. document the complete test environment and transport configuration
5. capture median, 95th-percentile, and 99th-percentile latency results
6. capture jitter and controller-delivery anomalies
7. demonstrate disconnect behavior that does not leave controls stuck
8. record whether the proposed direct seat-to-runtime data path should be accepted, revised, or rejected

### Phase 1B — Isolation

The demonstration shall:

1. launch two independent MAME sessions
2. assign separate virtual controllers and working directories
3. prove that each seat controls only its assigned session
4. force-stop one emulator process
5. show that the other session continues operating
6. clean up the failed session's processes, controllers, and temporary resources

### Phase 1C — Orchestration

The demonstration shall:

1. start from a clean documented environment
2. register one seat and one runtime host
3. browse a minimal legal test catalog
4. request and allocate a session
5. launch and play one MAME title
6. expose diagnosable lifecycle state
7. detect normal termination or runtime loss
8. return the seat to browsing without rebooting

## Product MVP acceptance statement

The Product MVP is not complete until multiple seats can discover active sessions, view an efficient preview, inspect meaningful player positions, start or join a compatible game, reconnect within a grace period, and spectate without operator intervention.
