# Runtime-Host Registration Validation

## Result

**PASS — 15 assertions on 2026-10-04.**

The validated implementation is commit `8a5071f`. The result bundle on the
reference runtime host is:

```text
/tmp/4play-runtime-host-smoke-20261004-172217
```

The repeatable command is:

```bash
cd ~/src/4-play
cargo build -p control-plane-server -p runtime-host-agent
bash tools/runtime-host-registration-smoke.sh
```

## Behavior validated

The harness starts a real control-plane process and runtime-host agent on the
Debian reference host. It verifies:

1. The control plane becomes healthy on an isolated loopback port.
2. The agent registers a stable host identity.
3. Two recurring, ordered heartbeats are accepted.
4. Host lookup and deterministic host listing both expose the registration.
5. Linux, CPU count, memory, H.264 encoders, and MAME are discovered.
6. SIGTERM shuts down the agent and control plane cleanly.
7. The control-plane TCP listener is released.

All 15 assertions passed.

## Reference-host capabilities

The agent reported:

| Capability | Value |
| --- | --- |
| Operating system | `linux` |
| Architecture | `x86_64` |
| Logical CPUs | 4 |
| Memory | 16,659,849,216 bytes |
| H.264 encoders | `h264_qsv`, `h264_vaapi`, `libx264` |
| Emulator adapters | `mame` |
| Agent version | `0.1.0` |

The final observed heartbeat sequence was 2 and the host status was `online`.

## Configuration

The agent accepts these environment variables:

| Variable | Default | Purpose |
| --- | --- | --- |
| `FOURPLAY_CONTROL_PLANE_URL` | `http://127.0.0.1:8080` | Control-plane base URL |
| `FOURPLAY_RUNTIME_HOST_ID` | lowercase system hostname | Stable host identity |
| `FOURPLAY_RUNTIME_HOST_NAME` | system hostname | Operator-facing name |
| `FOURPLAY_HEARTBEAT_SECONDS` | `5` | Heartbeat and retry interval |
| `FOURPLAY_MAME_PATH` | automatic | Optional MAME executable override |

The agent retries registration when the control plane is unavailable. If a
heartbeat fails, it refreshes registration before resuming. Re-registration
reads the last server-side sequence so an agent restart continues with a newer
heartbeat rather than sending stale state.

## Current limitation

The control plane stores registrations only in memory, so restarting it clears
the host list. The agent recovers automatically by registering again, but
durable history and offline-host detection require the next SQLite-backed
storage slice.

The agent currently reports zero active sessions. That count will be connected
to runtime-manager state when session allocation and supervision move behind
the control plane.
