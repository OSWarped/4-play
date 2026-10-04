# Runtime-Host Registration Validation

## Result

**PASS — 22 assertions on 2026-10-04.**

The validated persistence implementation is commit `ecbc980`. The result bundle on the
reference runtime host is:

```text
/tmp/4play-runtime-host-smoke-20261004-173241
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
7. Heartbeat expiry marks the stopped host offline.
8. SQLite retains the host, offline status, and heartbeat sequence across a
   real control-plane restart.
9. The restarted agent resumes at the next sequence and returns the host online.
10. The control-plane TCP listener is released.

All 22 assertions passed.

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

## Persistence and liveness

The control plane stores registrations in SQLite. The database path is selected
with `FOURPLAY_CONTROL_PLANE_DATABASE` and defaults to
`data/control-plane.sqlite3`. `FOURPLAY_RUNTIME_HOST_OFFLINE_SECONDS` controls
the heartbeat deadline and defaults to 15 seconds.

The live test used a two-second deadline. After the agent stopped, the host
became `offline`; the server restarted against the same database; and the host
remained present with heartbeat sequence 2. The agent then restarted, resumed
with sequence 3, and returned the host to `online`.

## Current limitation

The agent currently reports zero active sessions. That count will be connected
to runtime-manager state when session allocation and supervision move behind
the control plane.
