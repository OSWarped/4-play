# Phase 3 Soak Runbook

Use this when running the reference table for an extended family/party session.
The goal is to prove that the table can be used normally, recovered
predictably, and diagnosed afterward.

## Before the session

On the Linux server:

```bash
cd ~/src/4-play
git pull origin main

FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
tools/phase-3-cleanup-sessions.sh --smoke
```

Confirm strict idle readiness:

```bash
FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
FOURPLAY_PHASE3_STRICT_IDLE=1 \
FOURPLAY_PHASE3_STRICT_METADATA=1 \
tools/phase-3-systemd-smoke.sh
```

On each Windows seat PowerShell:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
```

Then launch the numbered seat:

```powershell
.\deploy\windows\Start-4Play-Seat1.ps1
.\deploy\windows\Start-4Play-Seat2.ps1
.\deploy\windows\Start-4Play-Seat3.ps1
.\deploy\windows\Start-4Play-Seat4.ps1
```

## During the session

Record a short note for each event:

| Time | Event | Expected result | Actual result |
| --- | --- | --- | --- |
| | Seat starts a new game | Game launches and media opens | |
| | Another seat joins | Seat controls only assigned player | |
| | Seat leaves with `Esc` | Other players keep running | |
| | Seat rejoins same slot | Slot reconnects cleanly | |
| | Spectator joins with `s1` | Media opens without reserving a player | |
| | Game is stopped | Session disappears or becomes terminal | |

Suggested minimum coverage:

- at least one 4-player TMNT or Simpsons session
- at least one 2-player game
- at least one spectator while players are active
- at least one leave/rejoin while other seats keep playing
- at least one session stop followed by a new game launch

## After the session

Collect diagnostics before cleanup if anything behaved oddly:

```bash
cd ~/src/4-play
tools/phase-3-diagnostics.sh
```

Clean up sessions:

```bash
FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
tools/phase-3-cleanup-sessions.sh --smoke
```

Run final strict smoke:

```bash
FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
FOURPLAY_PHASE3_STRICT_IDLE=1 \
FOURPLAY_PHASE3_STRICT_METADATA=1 \
tools/phase-3-systemd-smoke.sh
```

## Pass criteria

- the server remains reachable throughout the session
- all seats can start, join, leave, rejoin, and spectate predictably
- active seats keep playing when another seat exits
- cleanup returns the server to a strict smoke pass
- any observed issue has enough diagnostics to reproduce or investigate
