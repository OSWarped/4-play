# 4-Play Architecture

## Architectural thesis

4-Play uses a **central control plane** plus one or more **runtime hosts**.

Seats are thin clients from a product perspective: they do not own emulator state. The control plane coordinates sessions, while latency-sensitive controller input, video, and audio travel directly between a seat and its assigned runtime host whenever practical.

This preserves the terminal model without forcing every frame through the central API server.

## Major components

### Control-plane server

Responsibilities:

- device identity and registration
- game catalog and package metadata
- seat presence and capabilities
- session creation and lifecycle state
- player-slot reservation
- runtime-host selection
- authorization and short-lived connection grants
- active-session discovery
- operator APIs
- durable configuration and audit events

The control plane does not process normal controller packets or transcode every gameplay frame.

Status: not yet implemented.

### Runtime host agent and session runtime

Responsibilities:

- report CPU, GPU, encoder, controller, and storage capabilities
- validate and stage approved game packages
- create session-specific working directories
- create isolated virtual controllers
- launch and supervise emulator processes
- expose session media and input endpoints
- publish health and session events
- tear down processes and temporary resources

The first deployment may place the control plane and runtime host agent on one Linux machine. They remain separate logical components so additional Linux or Windows runtime hosts can be added later.

A first Rust implementation now exists at `runtime/session-runtime`.

### Validated session runtime path

The active development path is:

```text
Rust session runtime
  ├─ prepares session directories and FIFOs
  ├─ launches FFmpeg
  ├─ starts concurrent media readers and writers
  └─ launches headless MAME

Headless MAME
  ├─ raw BGR0 video FIFO
  └─ raw 48 kHz stereo S16LE audio FIFO
          ↓
Rust media bridge
  ├─ complete video-frame reads
  ├─ 20 ms audio-block reads
  ├─ bounded video queue
  ├─ bounded audio queue
  └─ queue and timing metrics
          ↓
FFmpeg
  ├─ libx264 ultrafast/zerolatency
  ├─ AAC audio
  └─ MPEG-TS over UDP
          ↓
Remote seat media player
```

This path avoids desktop, window, or screen capture. MAME emits media directly through custom raw-output options.

The validated runtime owns both MAME and FFmpeg. A one-command session launch has been demonstrated. Ctrl+C terminated the parent runtime and the observed child processes without leaving MAME or FFmpeg running.

### Earlier local KMSDRM validation

Before direct raw media output was established, the Linux host successfully launched MAME without X11 by using SDL's KMSDRM video backend:

```text
runtime or SSH shell
  → MAME
  → SDL
  → KMSDRM / DRM/KMS
  → GPU and local display
```

That experiment remains useful evidence that X11, a desktop environment, a window manager, and a graphical terminal are not required. It is not the current remote media path.

See [MAME KMSDRM Runtime Validation](../experiments/MAME_KMSDRM_VALIDATION.md).

### Game-specific media metadata

Raw video contains no self-describing frame boundary or refresh metadata. The runtime must know the correct width, height, pixel format, and refresh rate for each game.

Validated examples:

| Game | Resolution | Refresh rate |
| --- | --- | --- |
| Aliens | 288×224 | 59.185606 Hz |
| TMNT | 320×224 | 60.000000 Hz |
| Killer Instinct | 320×240 | 58.981183 Hz |

Encoding TMNT at the Aliens refresh rate caused video duration to be stretched and audio to appear ahead. Re-encoding at TMNT's actual 60 Hz refresh restored synchronization. Therefore, refresh rate is game metadata and must not be represented as one shared default.

The current CLI accepts width, height, and refresh rate explicitly. Automatic discovery from MAME metadata is planned.

### Concurrent-session validation

Two independent MAME sessions have run simultaneously with:

- separate working directories
- separate video and audio FIFOs
- separate Rust bridges
- separate encoders and UDP destinations
- different resolutions and refresh rates

Aliens and TMNT maintained independent media output. This validates media-side session isolation, but complete Phase 1B isolation still requires separate virtual controllers, no input leakage, abnormal termination tests, and save/NVRAM validation.

### Virtual controllers

A development tool at `tools/uinput-test` creates a Linux virtual controller through `/dev/uinput`.

Validated controller shape:

- two absolute axes: X and Y
- eight buttons
- Linux event-device handler
- Linux joystick handler

The controller has been verified with `jstest`. It has not yet been integrated into `session-runtime` or proven as MAME input.

Production runtime processes should use a dedicated service account with narrowly controlled access to required render, audio, and input devices. Membership in the Linux `input` group and a development udev rule are acceptable for current validation but require security review before deployment.

### Seat client

Responsibilities:

- attract mode
- catalog browsing
- active-session browsing
- live preview display
- player-slot selection
- game controls and operator-safe escape actions
- direct real-time connection to the assigned runtime host
- graceful handling of reconnect, host loss, and session termination

Status: VLC currently acts only as a media receiver for experiments. A real seat client has not yet been implemented.

### Operator client

Responsibilities:

- view seat and host health
- inspect active sessions
- drain or disable a runtime host
- terminate an unhealthy session
- manage approved game packages
- review latency and reliability telemetry

Status: not yet implemented.

## Control plane and data plane

### Control plane

Reliable request/response and event delivery handles device registration, catalog queries, session creation, slot reservations, lifecycle changes, operator actions, and connection negotiation. HTTP plus WebSocket or server-sent events is sufficient initially.

### Real-time data plane

Direct seat-to-runtime communication handles controller state, video, audio, and stream timing feedback.

The current media feasibility path uses unicast UDP MPEG-TS carrying H.264 and AAC. This is a development transport, not yet a permanent product standard.

The current input direction is state-oriented controller packets rather than isolated key-down and key-up events. The detailed transport remains unimplemented and must be validated for loss, jitter, disconnect neutralization, and latency.

## Session lifecycle

Canonical product states:

1. `requested`
2. `allocating`
3. `starting`
4. `ready`
5. `active`
6. `stopping`
7. `stopped`

Failure states include `allocation_failed`, `launch_failed`, `runtime_lost`, `unhealthy`, and `terminated`.

The current Rust runtime contains an early local state enum and process abstractions, but not all transitions are wired into durable orchestration. State transitions will ultimately be owned by the control plane while runtime facts originate from the runtime host. Commands must become idempotent.

## Player slots

A session exposes logical player slots defined by its game package. A slot may represent a generic player number, fixed cabinet position, fixed character, team side, or spectator access.

Slot reservations are leases. A disconnected seat receives a reconnect grace period before its slot becomes available.

## Live previews

Active sessions publish a low-cost preview stream or periodic preview frames. Preview availability must never block gameplay or require one full-quality encoder per browsing seat.

Evaluation order:

1. reuse one encoded session stream with low-rate subscribers
2. provide a low-bitrate simulcast layer
3. produce periodic JPEG or WebP frames
4. fall back to package artwork

## Deployment stages

### Phase 1

One Linux machine may run the control-plane server, database, runtime host agent, MAME, and capture/encoding processes. A seat client runs on separate hardware.

### Later

- multiple runtime hosts
- optional Windows runtime hosts
- dedicated control-plane server
- operator stations
- many seats and table types

## Persistence

Use a relational database for devices, game packages, runtime hosts, sessions, lifecycle events, player-slot reservations, configuration, and audit records. High-frequency controller and media packets are not persisted.

SQLite is acceptable for the first single-server proof of concept. PostgreSQL is preferred before multi-host or commercial deployment.

## Observability

Every session receives a correlation ID. Logs and metrics should include session ID, seat ID, runtime host ID, game package version, emulator adapter version, launch duration, stream setup duration, input packet loss, measurable encode/decode latency, disconnects, and recovery events.

The current media bridge already tracks received video frames, received audio blocks and samples, dropped video frames, dropped audio blocks, queue depth, first-arrival timing, and observed frame rate. These metrics are development instrumentation and are not yet exported to a monitoring system.

## Failure model

The design assumes processes, seats, networks, and hosts can fail independently.

Required behaviors:

- runtime heartbeat loss changes session health
- orphan emulator processes are reaped
- duplicate start and stop commands are safe
- stale slot leases expire
- a seat can return to browsing without rebooting
- one failed session does not terminate unrelated sessions

Current evidence confirms clean termination in the tested Ctrl+C path and no remaining MAME or FFmpeg process afterward. Broader abnormal termination and recovery tests remain.
