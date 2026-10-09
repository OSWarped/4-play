# Windows Seat Launchers

These scripts are the Phase 3 manual launchers for the reference table seats.
They assume:

- the repo is checked out at `C:\Users\blake\source\repos\4-play`
- `target\release\seat-input.exe` has been built
- FFplay is installed or discoverable through the current setup
- the Linux server is reachable at `http://192.168.20.68:8080`
- the seat media destination address is currently `192.168.20.10`

## Build the client

From PowerShell:

```powershell
cd C:\Users\blake\source\repos\4-play
cargo build --release -p seat-input
```

## Start seats

Open one PowerShell window per seat:

```powershell
.\deploy\windows\Start-4Play-Seat1.ps1
.\deploy\windows\Start-4Play-Seat2.ps1
.\deploy\windows\Start-4Play-Seat3.ps1
.\deploy\windows\Start-4Play-Seat4.ps1
```

For the current single-PC development setup, every launcher uses the same media
destination IP but a different `seat-id`. That is enough to test independent
player-slot ownership, joining, leaving, reconnecting, and spectating.

## Recommended Phase 3 manual test

1. Start seat 1 and launch TMNT.
2. Start seat 2 and join with `j1.2`.
3. Start seat 3 and join with `j1.3`.
4. Start seat 4 and either join with `j1.4` or spectate with `s1`.
5. Confirm each joined seat controls only its assigned player.
6. Press `Esc` in one seat and confirm the other seats keep running.
7. Rejoin the same slot from the disconnected seat.
8. Stop all seats and confirm the Linux services stay active.

On the Linux server:

```bash
cd ~/src/4-play
tools/phase-3-systemd-smoke.sh
```

Use strict idle mode only after all sessions have been stopped:

```bash
FOURPLAY_PHASE3_STRICT_IDLE=1 tools/phase-3-systemd-smoke.sh
```

