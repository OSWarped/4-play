# Control-Plane Foundation

## Scope

This is the first Phase 1C implementation slice. It establishes a runnable HTTP
service, a shared versioned Rust protocol crate, a SQLite-backed runtime-host
registry, and a runtime-host agent. It does not yet allocate a session or
launch MAME through the control plane.

## Run

```powershell
cargo run -p control-plane-server
```

The service binds to `127.0.0.1:8080` by default. Override it with
`FOURPLAY_CONTROL_PLANE_BIND`, for example:

```powershell
$env:FOURPLAY_CONTROL_PLANE_BIND = "0.0.0.0:8080"
cargo run -p control-plane-server
```

The loopback default avoids unintentionally exposing an unauthenticated
development API to the network.

## Endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/health` | Process liveness |
| `GET` | `/ready` | Service readiness |
| `GET` | `/api/v1` | Service and API version metadata |
| `GET` | `/api/v1/runtime-hosts` | Deterministically ordered host list |
| `PUT` | `/api/v1/runtime-hosts/{host_id}` | Idempotent registration or capability refresh |
| `GET` | `/api/v1/runtime-hosts/{host_id}` | Registered-host lookup |
| `POST` | `/api/v1/runtime-hosts/{host_id}/heartbeat` | Idempotent sequenced heartbeat |

Registration validates stable host identity, display name, agent version, and
logical CPU count. A new registration returns HTTP 201; an update returns HTTP
200. Heartbeat sequence numbers prevent an older packet from overwriting newer
host state. An exact retry is accepted, while a reused sequence carrying
different data returns HTTP 409.

The registry is persisted in SQLite. Host reads apply the configured heartbeat
deadline, and expired hosts become `offline` until registration or a valid
heartbeat returns them to `online`.

## Shared protocol

`shared/control-protocol` owns:

- the public API version
- service health payloads
- canonical serialized session states
- runtime-host capabilities
- registration, heartbeat, list, and status payloads
- structured API errors

The canonical session states are:

```text
requested
allocating
starting
ready
active
stopping
stopped
allocation_failed
launch_failed
runtime_lost
unhealthy
terminated
```

## Validation

```powershell
cargo test -p control-protocol -p control-plane-server
```

Thirteen automated tests cover protocol serialization, lifecycle semantics,
health, readiness, API metadata, host validation, idempotent registration,
listing, heartbeat retries, stale and conflicting sequence rejection, missing
hosts, and unknown routes.

A live smoke test also compiled and started the real server on loopback port
18080, registered `reference-linux`, recorded heartbeat sequence 1, listed one
host, and received `ok` health before stopping the process.

## Next slice

Implement the minimal legal catalog and derive validated runtime profiles from
MAME metadata.
