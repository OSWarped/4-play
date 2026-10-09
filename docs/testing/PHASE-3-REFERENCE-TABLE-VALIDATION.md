# Phase 3 Reference Table Validation

Date: 2026-10-09

Status: **manual four-seat flow accepted on the reference setup**.

## Environment

| Role | Value |
| --- | --- |
| Linux runtime/control host | `192.168.20.68` |
| Windows seat/client host | `192.168.20.10` |
| Control plane | `http://192.168.20.68:8080` |
| Seat IDs | `windows-seat-1` through `windows-seat-4` |
| Server services | `4play-control-plane.service`, `4play-runtime-host-agent.service` |

The Windows launchers in `deploy/windows/` were tested after temporarily
enabling script execution for each PowerShell process:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
```

## Validated flow

The following player-facing flow was manually tested successfully:

1. Seat 1 launched from `Start-4Play-Seat1.ps1`.
2. Seat 1 started a TMNT session.
3. Seat 2 launched from `Start-4Play-Seat2.ps1` and joined with `j1.2`.
4. Seat 3 launched from `Start-4Play-Seat3.ps1` and joined with `j1.3`.
5. Seat 4 launched from `Start-4Play-Seat4.ps1` and joined or spectated.
6. Joined seats controlled their own assigned player slots.
7. Leaving and returning to the browser did not kill unrelated seats.
8. The launcher wrapper accepted the default reference settings and started
   cleanly after the optional FFplay-path default was fixed.

## Current validation commands

Before testing:

```bash
cd ~/src/4-play
FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
tools/phase-3-cleanup-sessions.sh --smoke
```

After testing:

```bash
cd ~/src/4-play
FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
FOURPLAY_PHASE3_STRICT_IDLE=1 \
FOURPLAY_PHASE3_STRICT_METADATA=1 \
tools/phase-3-systemd-smoke.sh
```

Expected strict smoke target:

```text
Phase 3 systemd smoke summary: 15 passed, 0 warned, 0 failed
```

## Manual soak validation

Date: 2026-10-09

Status: **accepted**.

Artifact directory on the Linux server:

```text
/tmp/4play-phase-3-soak-20261009-175455-manual
```

Soak report:

```text
samples=19
max_active_sessions=1
host_statuses=reference-linux:online:19
active_state_samples=active:1:15
sample_error_files=0
diagnostics_errors=none
```

Post-run cleanup found no non-terminal sessions, and final strict Phase 3 smoke
passed:

```text
Phase 3 systemd smoke summary: 15 passed, 0 warned, 0 failed
```

This validates the monitor/report/cleanup path around a real manual session and
confirms the reference server recovered to a strict idle pass afterward.

## Remaining Phase 3 validation

The remaining Phase 3 checks are physical-table observations that should be
recorded when the hardware layout is final:

- physical control panel ergonomics
- audio routing and isolation at the table
- recovery behavior during longer family/party use
