# Phase 3 Reference Table Operations

Phase 3 turns the validated shared-session MVP into a repeatable four-seat
reference table. The goal is not the final polished cabinet UI yet; it is a
stable, restartable operating setup that can be installed, diagnosed, and used
without remembering the development commands.

## Reference topology

The current reference deployment is:

| Role | Address / identity | Notes |
| --- | --- | --- |
| Runtime/control server | `192.168.20.68` | Linux host running control-plane, runtime-host-agent, MAME, and FFmpeg |
| Seat 1 | `windows-seat-1` | Windows client, media destination currently `192.168.20.10` |
| Seat 2 | `windows-seat-2` | Windows client, can join or spectate active sessions |
| Seat 3 | `windows-seat-3` | Planned third player/client |
| Seat 4 | `windows-seat-4` | Planned fourth player/client |

Phase 3 assumes wired LAN play. If a seat's IP changes, update the matching
launcher or set `FOURPLAY_SEAT_ADDRESS` before starting the client.

The visible game list is intentionally curated. Do not add every verified MAME
clone or regional variant to the player-facing catalog; prefer one entry per
game using the US-first, World-second rule in
[Catalog Curation](../design/CATALOG-CURATION.md).

## Standard ports

The reference host should reserve a broad enough port pool for several sessions,
joined seats, spectators, and preview traffic:

| Purpose | Default range |
| --- | --- |
| Control plane HTTP | `8080/tcp` |
| Media streams | `41000-41099/udp` |
| Seat input | `42000-42099/udp` |

The control plane owns allocation from these pools. Seats should not hard-code a
media port when using the control-plane workflow; they receive session and
viewer grants from the server.

## Server startup target

The Phase 3 server should run these long-lived processes:

1. `control-plane-server`
2. `runtime-host-agent`

The runtime-host agent launches `session-runtime`, MAME, and FFmpeg per active
session. Those child processes should disappear when a session stops.

Systemd templates are provided in `deploy/systemd/`:

- `4play-control-plane.service`
- `4play-runtime-host-agent.service`
- `4play.env.example`
- `install-reference-server.sh`

Install them on the Linux server only after reviewing the tokens and paths.

```bash
cd ~/src/4-play
sudo deploy/systemd/install-reference-server.sh
sudo nano /etc/4play/4play.env
sudo systemctl enable --now 4play-control-plane.service
sudo systemctl enable --now 4play-runtime-host-agent.service
```

The reference environment should point at the checked-in test catalog unless a
site-specific catalog has been created:

```text
FOURPLAY_CATALOG_PATH=/home/blake/src/4-play/catalog/test-catalog.json
```

## Windows seat startup target

Windows launchers are provided in `deploy/windows/`:

- `Start-4Play-Seat1.ps1`
- `Start-4Play-Seat2.ps1`
- `Start-4Play-Seat3.ps1`
- `Start-4Play-Seat4.ps1`

Each script starts `seat-input.exe` with a stable `--seat-id`, the reference
control plane, the seat token, and the local media destination address.

The scripts are intentionally manual first. After the four-seat behavior is
stable, they can be registered as Windows startup tasks.

## Diagnostics

For a pass/fail readiness check of the systemd-managed reference server, run:

```bash
cd ~/src/4-play
tools/phase-3-systemd-smoke.sh
```

Run this on the Linux server:

```bash
cd ~/src/4-play
tools/phase-3-diagnostics.sh
```

The report checks:

- service/process status
- control-plane health/readiness
- active sessions
- runtime host registration
- configured games
- relevant TCP/UDP listeners
- active MAME, FFmpeg, and session-runtime children
- `/dev/uinput` permissions
- 4-Play virtual input devices
- recent control-plane and runtime-host-agent journal entries
- recent host-agent logs

For a machine-readable catalog snapshot:

```bash
FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin report --json
```

After moving to the systemd-backed database, restore the current reference
metadata if the report shows incomplete catalog entries:

```bash
FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin seed-known-metadata

FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin seed-placeholders \
  --asset-root /home/blake/src/4-play/data/assets \
  --update-metadata
```

## Acceptance target

Phase 3 exits when the reference table can:

- boot the Linux server into a ready state without manual process launch
- start four Windows seats from stable launchers
- start, join, leave, rejoin, and spectate sessions predictably
- recover from a stopped seat without corrupting player-slot state
- recover from stopped MAME/FFmpeg/session-runtime children without affecting
  unrelated sessions
- run a multi-hour family/party play session with predictable cleanup
- produce a useful diagnostics report when something goes wrong
