# Phase 2 Shared-Session Validation

Date: 2026-10-09

Status: **Phase 2 exit accepted on the reference host.** Automated smoke and
manual Windows multi-seat validation both passed.

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
4. catalog-admin JSON metadata report, known-metadata seeding, placeholder
   asset seeding, authenticated asset validation, and complete browser metadata
   report
5. the seat browser renders seeded descriptions, control notes, media badges,
   and primary image URLs without launching a game
6. seat 1 starts a TMNT session
7. runtime host launches the session and reports it active
8. active-session discovery exposes a fresh still preview
9. the seat browser renders active-session slots, seeded player labels, and
   preview details
10. the preview BMP is served through the authenticated asset endpoint
11. old still previews are labeled `stale_available`
12. seat 2 reserves player 2 atomically
13. seat 2 connects, disconnects into a reconnectable lease, reconnects, and
    releases player 2 back to open
14. a spectator receives a distinct media-port grant
15. active-session discovery reports spectator count
16. runtime assignments expose spectator media ports for the running session
17. the spectator grant releases without affecting player slots
18. the shared session stops cleanly and reaps its runtime process
19. the agent and control plane exit cleanly

The accepted automated run produced 42 passing assertions:

```text
Results: /tmp/4play-phase-2-shared-session-smoke-20261009-083322
OVERALL  PASS
```

The harness is intentionally focused on orchestration and safety. It does not
replace manual cabinet/play validation for input feel, local display layout, or
seat ergonomics.

## Reproduce the accepted Phase 2 state

These commands rebuild the current reference deployment, verify catalog/admin
metadata, and run the same Windows multi-seat validation that accepted Phase 2.

### 1. Update and build on the Debian reference host

From Windows PowerShell:

```powershell
ssh blake@192.168.20.68
```

On the Debian host:

```bash
cd ~/src/4-play
git pull --ff-only origin main
source ~/.cargo/env
cargo build --release \
  -p control-plane-server \
  -p runtime-host-agent \
  -p session-runtime \
  -p catalog-admin \
  -p seat-input
```

### 2. Run the isolated automated Phase 2 smoke

```bash
cd ~/src/4-play
tools/phase-2-shared-session-smoke.sh
```

Expected ending:

```text
OVERALL  PASS
```

The accepted run was:

```text
Results: /tmp/4play-phase-2-shared-session-smoke-20261009-083322
OVERALL  PASS
```

### 3. Validate the live control-plane catalog and asset cache

The live development services use:

- control plane: `http://192.168.20.68:8080`
- local host URL: `http://127.0.0.1:8080`
- seat token: `phase-1c-seat-token-2026`
- configured asset root: `/home/blake/src/4-play/data/assets`

Run on the Debian host:

```bash
cd ~/src/4-play

FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin report

FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin report --json | python3 -m json.tool

FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin validate-assets

FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin validate-assets --json | python3 -m json.tool
```

Expected human-readable results:

```text
Library metadata report
  games: 4
  complete: 4
  incomplete: 0

Validated 16 metadata asset paths.
```

If the live catalog ever loses metadata or placeholder assets, restore the
accepted development state with:

```bash
cd ~/src/4-play

FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin seed-known-metadata

FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin seed-placeholders \
  --asset-root /home/blake/src/4-play/data/assets \
  --update-metadata
```

Do not seed `/opt/4play/assets` for this reference deployment unless the
control-plane service is also started with `FOURPLAY_ASSET_ROOT=/opt/4play/assets`.

### 4. Update and build the Windows seat client

From Windows PowerShell:

```powershell
cd C:\Users\blake\source\repos\4-play
git pull --ff-only origin main
cargo build --release -p seat-input
```

If `target\release\seat-input.exe` is locked by a running client, close the
client or build to a temporary target directory:

```powershell
cargo build --release -p seat-input --target-dir target\seat-input-validation-build
```

Then substitute this executable path in the commands below:

```powershell
.\target\seat-input-validation-build\release\seat-input.exe
```

### 5. Validate browser-only catalog rendering from Windows

```powershell
.\target\release\seat-input.exe `
  --control-plane http://192.168.20.68:8080 `
  --api-token phase-1c-seat-token-2026 `
  --seat-id windows-seat-1 `
  --destination-ip 192.168.20.10 `
  --list-only
```

Expected browser content includes:

```text
Available games:
about:
controls:
media: artwork, marquee, screenshot, logo
primary image:
```

### 6. Start a shared TMNT session from seat 1

```powershell
.\target\release\seat-input.exe `
  --control-plane http://192.168.20.68:8080 `
  --api-token phase-1c-seat-token-2026 `
  --seat-id windows-seat-1 `
  --destination-ip 192.168.20.10
```

Choose TMNT from the numbered game list.

### 7. Validate active-session browser rendering from seat 2

In a second PowerShell:

```powershell
cd C:\Users\blake\source\repos\4-play

.\target\release\seat-input.exe `
  --control-plane http://192.168.20.68:8080 `
  --api-token phase-1c-seat-token-2026 `
  --seat-id windows-seat-2 `
  --destination-ip 192.168.20.10 `
  --list-only
```

Expected active-session content includes:

```text
Active sessions:
Teenage Mutant Ninja Turtles
P1 Leonardo occupied by windows-seat-1
P2 Michelangelo open
P3 Donatello open
P4 Raphael open
preview:
```

### 8. Join the shared TMNT session from seat 2

```powershell
.\target\release\seat-input.exe `
  --control-plane http://192.168.20.68:8080 `
  --api-token phase-1c-seat-token-2026 `
  --seat-id windows-seat-2 `
  --destination-ip 192.168.20.10
```

At the prompt, join player 2:

```text
j1.2
```

Validate manually:

- seat 1 controls Leonardo / player 1
- seat 2 controls Michelangelo / player 2
- simultaneous input works
- controls route only to the joined player
- input latency still feels tight

### 9. Open spectator mode from seat 3

In a third PowerShell:

```powershell
cd C:\Users\blake\source\repos\4-play

.\target\release\seat-input.exe `
  --control-plane http://192.168.20.68:8080 `
  --api-token phase-1c-seat-token-2026 `
  --seat-id windows-seat-3 `
  --destination-ip 192.168.20.10
```

At the prompt, spectate the active session:

```text
s1
```

Validate manually:

- spectator media opens on its own media port
- spectator mode does not reserve a player slot
- spectator media does not affect player input latency
- active-session browsing reports spectator count

### 10. Stop and recovery check

Press `Esc` in the playing seat clients to leave/stop as appropriate, then run:

```powershell
.\target\release\seat-input.exe `
  --control-plane http://192.168.20.68:8080 `
  --api-token phase-1c-seat-token-2026 `
  --seat-id windows-seat-1 `
  --destination-ip 192.168.20.10 `
  --list-only
```

Expected:

- stopped sessions no longer appear as active
- browsing returns normally
- no FFplay or seat-input window remains unexpectedly stuck

## Accepted manual validation

Manual validation passed on 2026-10-09 from Windows clients against the Debian
reference host.

Accepted observations:

1. browser-only catalog rendering showed seeded descriptions, control notes,
   media badges, and primary image URLs
2. TMNT launched from `windows-seat-1`
3. `windows-seat-2` joined the active TMNT session as player 2 with `j1.2`
4. active-session browsing showed Leonardo, Michelangelo, Donatello, and
   Raphael slot labels and preview information
5. controls routed independently to the joined player slots
6. spectator mode opened from a third seat with `s1`
7. spectator mode did not reserve a player slot or disrupt gameplay
8. preview/catalog metadata rendering did not introduce a noticeable gameplay
   regression

## Historical manual checklist

The live manual pass used this checklist:

1. start TMNT from seat 1
2. join player 2 from seat 2
3. disconnect and reconnect seat 2
4. open spectator mode from seat 3
5. confirm the active-session list shows player slots, spectator count, and
   preview status
6. confirm controls route only to the joined player
7. confirm spectator media does not affect gameplay latency
8. stop the session and confirm browsing recovers on every seat
