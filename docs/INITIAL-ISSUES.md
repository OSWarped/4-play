# Recommended Initial GitHub Issues

## Phase 1

1. **Measure local and streamed MAME latency** — record local baseline and remote added latency with repeatable methodology.
2. **Bootstrap the control-plane service** — add health, configuration, structured logging, and local persistence.
3. **Implement runtime-host registration and heartbeat** — register capabilities and detect host loss.
4. **Define the versioned session protocol** — specify create, start, ready, active, stop, and failure messages.
5. **Implement the MAME runtime adapter** — launch an approved machine from typed package configuration.
6. **Create session-specific virtual controllers** — guarantee controller isolation and neutral state on disconnect.
7. **Prototype direct seat-to-runtime input transport** — measure update rate, jitter, loss, and reconnect behavior.
8. **Prototype low-latency video and audio transport** — compare candidate transports on the wired LAN.
9. **Build the minimal seat client** — browse the catalog, launch a session, display media, and send controls.
10. **Prove two-session isolation** — test controller, process, save, and temporary-file separation.
11. **Implement runtime crash detection and cleanup** — detect forced emulator termination and recover the seat.

## Phase 2 preparation

12. **Design the active-session preview strategy** — measure preview approaches without adding one full encoder per viewer.
13. **Implement player-slot reservation leases** — support atomic reservation, reconnect grace, and release.
14. **Model fixed-character and positioned cabinets** — validate the slot model using representative arcade games.
