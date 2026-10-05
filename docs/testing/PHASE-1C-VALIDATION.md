# Phase 1C Validation

Date: 2026-10-04

Status: **PASS — Phase 1C complete on the reference host.**

## Exit criterion

A seat must browse the legal test catalog, request a session without knowing
emulator arguments, connect directly to the assigned runtime, play, and return
to browsing after both normal termination and runtime loss.

## Automated evidence

Run from the repository on the Debian reference host after building release
binaries:

```bash
tools/phase-1c-smoke.sh
```

The harness uses an isolated SQLite database, TCP port 41810, a unique runtime
state directory, and only the session PIDs it created. It verifies:

1. release binaries and `/dev/uinput` access
2. control-plane readiness and verified TMNT catalog browsing
3. rejection of unauthenticated control-plane requests
4. transactional session allocation and host assignment
5. automatic `session-runtime`, FFmpeg, controller, and MAME launch
6. readiness only after real video and audio reach the runtime bridge
7. durable `active`, `stopping`, and `stopped` transitions
8. orderly process reaping after a normal stop
9. a second launch without rebooting either endpoint
10. injected MAME failure and durable `runtime_lost` diagnosis
11. continued host health and catalog browsing after that failure
12. the real seat client browsing, requesting, sending grant-authenticated UDP
    input, stopping, and returning to browsing
13. clean agent and control-plane shutdown

The accepted authenticated run produced 32 passing assertions:

```text
Results: /tmp/4play-phase-1c-smoke-20261004-221202
OVERALL  PASS
```

The same revision passed the complete Linux workspace test suite, strict
Clippy with warnings denied, and release builds for the control plane, runtime
host agent, session runtime, and seat client.

## Components delivered

- SQLite runtime-host, catalog, session, grant, and lifecycle-event state
- online-host selection with seat and UDP-port conflict prevention
- explicit runtime assignments and idempotent legal state transitions
- capture-capable MAME path and INI adapter configuration
- agent process supervision, safe PID adoption, stop, and runtime-loss reporting
- cancellation-aware FIFO readers and media-confirmed runtime readiness
- version 2 controller packets authenticated by the session grant token
- interactive and one-shot seat catalog/request/connect/stop workflows

## Reference deployment

On the Linux host:

```bash
cd ~/src/4-play
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release \
  -p control-plane-server \
  -p runtime-host-agent \
  -p session-runtime

FOURPLAY_CONTROL_PLANE_BIND=0.0.0.0:8080 \
FOURPLAY_SEAT_API_TOKEN='<provisioned-seat-token>' \
FOURPLAY_RUNTIME_HOST_API_TOKEN='<provisioned-host-token>' \
  ./target/release/control-plane-server
```

In a second shell:

```bash
cd ~/src/4-play
FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_RUNTIME_HOST_ID=reference-linux \
FOURPLAY_RUNTIME_HOST_ADDRESS=192.168.20.68 \
FOURPLAY_RUNTIME_HOST_API_TOKEN='<provisioned-host-token>' \
FOURPLAY_MAME_PATH="$HOME/src/mame-4play/mame" \
FOURPLAY_MAME_INI_PATH=/opt/4play/config/mame \
FOURPLAY_SESSION_RUNTIME_PATH="$PWD/target/release/session-runtime" \
  ./target/release/runtime-host-agent
```

Before launching seats from another machine, confirm the runtime host firewall
allows the orchestrated ranges from the seat IP. For the reference Windows seat
at `192.168.20.10`:

```bash
sudo ufw allow from 192.168.20.10 to any port 8080 proto tcp \
  comment '4-play control plane'
sudo ufw allow from 192.168.20.10 to any port 41000:41099 proto udp \
  comment '4-play media range'
sudo ufw allow from 192.168.20.10 to any port 42000:42099 proto udp \
  comment '4-play seat input range'
```

Seat input ports start at `42000` and increment per active seat. Allowing only
`42000/udp` lets seat 1 work but blocks seat 2 on `42001/udp` and seat 3 on
`42002/udp`.

On the Windows seat, set the installed FFplay path when it is not on `PATH`:

```powershell
$env:FOURPLAY_FFPLAY_PATH = "C:\Users\blake\AppData\Local\Microsoft\WinGet\Packages\Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe\ffmpeg-9.0-full_build\bin\ffplay.exe"
$env:FOURPLAY_SEAT_API_TOKEN = '<provisioned-seat-token>'
.\target\release\seat-input.exe `
  --control-plane http://192.168.20.68:8080 `
  --seat-id windows-seat-1 `
  --destination-ip 192.168.20.10
```

Choose a catalog title, play with the documented keyboard mapping, and press
Escape to request an orderly stop and return to the catalog.

## Boundary after Phase 1C

Phase 2 begins with shared sessions: active-session discovery, previews,
meaningful player slots, atomic join/reconnect leases, and spectator mode.
Those capabilities are intentionally not part of the Phase 1C exit.
