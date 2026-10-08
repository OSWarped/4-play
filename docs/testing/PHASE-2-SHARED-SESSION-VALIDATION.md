# Phase 2 Shared-Session Validation

Date: 2026-10-08

Status: **in progress — repeatable smoke harness added.**

## Exit criterion

Multiple seats can discover what is happening, inspect available positions,
start or join a game, and spectate without operator intervention. Preview
availability or failure must not block starting, joining, spectating, or
continuing gameplay.

## Automated smoke harness

Run from the repository on the Debian reference host after building release
binaries:

```bash
tools/phase-2-shared-session-smoke.sh
```

The harness starts an isolated control plane and runtime-host agent using:

- TCP port `41820`
- media port pool starting at `43100`
- input port pool starting at `44100`
- an isolated SQLite database
- an isolated runtime state directory
- an isolated asset root with `previews/`

It verifies the current Phase 2 product flow:

1. release binaries and `/dev/uinput` access
2. isolated control-plane readiness
3. legal catalog browsing
4. seat 1 starts a TMNT session
5. runtime host launches the session and reports it active
6. active-session discovery exposes a fresh still preview
7. the preview BMP is served through the authenticated asset endpoint
8. old still previews are labeled `stale_available`
9. seat 2 reserves player 2 atomically
10. seat 2 connects, disconnects into a reconnectable lease, reconnects, and
    releases player 2 back to open
11. a spectator receives a distinct media-port grant
12. active-session discovery reports spectator count
13. runtime assignments expose spectator media ports for the running session
14. the spectator grant releases without affecting player slots
15. the shared session stops cleanly and reaps its runtime process
16. the agent and control plane exit cleanly

The harness is intentionally focused on orchestration and safety. It does not
replace manual cabinet/play validation for input feel, local display layout, or
seat ergonomics.

## Manual validation still required

Before Phase 2 exit, run a live multi-seat pass from Windows clients:

1. start TMNT from seat 1
2. join player 2 from seat 2
3. disconnect and reconnect seat 2
4. open spectator mode from seat 3
5. confirm the active-session list shows player slots, spectator count, and
   preview status
6. confirm controls route only to the joined player
7. confirm spectator media does not affect gameplay latency
8. stop the session and confirm browsing recovers on every seat

Record the accepted run's result directory and any manual observations here
when Phase 2 is ready to close.
