# Session Runtime and Media Validation

## Purpose

This document records the first working 4-Play session runtime and the evidence gathered while validating headless MAME, synchronized media, concurrent sessions, CHD-backed games, and Linux virtual-controller creation.

The experiments establish important feasibility evidence. They do not complete remote play because seat-to-runtime input and objective button-to-photon latency measurements remain.

## Validated host and client

- Runtime host: Linux server at `192.168.20.68`
- Development seat: Windows machine at `192.168.20.10`
- Network: wired `192.168.20.0/24` LAN
- Emulator: custom MAME build in `~/src/mame-4play`
- Runtime: Rust workspace crate `runtime/session-runtime`
- Media receiver: VLC on Windows
- Encoder: FFmpeg using libx264 and AAC

A complete hardware and utilization inventory still belongs in `docs/testing/REFERENCE-ENVIRONMENT.md`.

## Custom MAME outputs

The custom MAME build exposes:

- `-rawvideowrite <path>` for raw BGR0 video frames
- `-rawaudiowrite <path>` for raw PCM audio

The validated audio format is:

- signed 16-bit little endian
- 48,000 Hz
- two channels

The raw streams are written to session-specific FIFOs.

## Session runtime responsibilities

The current Rust runtime:

1. parses session and destination parameters
2. prepares `/tmp/4play/session-<id>`
3. creates session-specific configuration, NVRAM, state, snapshot, and diff directories
4. creates raw video and audio FIFOs
5. launches one FFmpeg encoder child process
6. starts concurrent Rust readers and encoder writers
7. launches one headless MAME child process
8. waits for MAME and owns the encoder lifecycle

The repository is a Cargo workspace containing:

- `runtime/session-runtime`
- `tools/uinput-test`

## Media pipeline

```text
MAME raw video FIFO ─┐
                     ├─> Rust media bridge ─> FFmpeg ─> UDP MPEG-TS ─> VLC
MAME raw audio FIFO ─┘
```

Video is read as complete frames. Audio is read in 20 ms blocks. The bridge uses bounded queues so encoder backpressure cannot grow into an unbounded latency buffer. When the video queue is full, stale video may be discarded.

The FFmpeg development profile uses:

- raw BGR0 video input
- raw 48 kHz stereo S16LE audio input
- libx264
- `ultrafast` preset
- `zerolatency` tuning
- no B-frames
- AAC at 128 kb/s
- MPEG-TS over unicast UDP

High-numbered UDP ports such as `41004` are preferred during development because port `5004` conflicted with the Windows Media Player Network Sharing Service on the test seat.

## Synchronization validation

### Aliens

- resolution: 288×224
- refresh: 59.185606 Hz
- captured audio and video appeared synchronized
- approximately 60 seconds of capture differed by roughly 31 ms in calculated duration

### TMNT

- resolution: 320×224
- refresh: 60.000000 Hz

TMNT initially appeared to have audio ahead of video. The raw capture contained 3,602 video frames and 60 seconds of audio. Interpreting those frames at the Aliens refresh rate stretched video to about 60.86 seconds. MAME metadata confirmed TMNT is exactly 60 Hz. Re-encoding at 60 Hz restored synchronization.

Conclusion: resolution and refresh rate are per-game metadata. A shared default is unsafe.

### Killer Instinct

- resolution: 320×240
- refresh: 58.981183 Hz
- ROM ZIP plus `kinst/kinst.chd`
- synchronized live output observed

Killer Instinct validated CHD-backed launch, a third resolution/refresh combination, and a more demanding game and audio configuration.

## Concurrent-session validation

Aliens and TMNT were run simultaneously with independent:

- session IDs
- working directories
- video FIFOs
- audio FIFOs
- Rust bridges
- FFmpeg processes
- UDP destination ports

Both streams were captured independently. Aliens remained synchronized. TMNT became synchronized after using its correct 60 Hz refresh metadata.

This demonstrates media-side concurrent-session feasibility. It does not yet prove virtual-controller, save, NVRAM, or failure isolation.

## Process lifecycle validation

The runtime now launches MAME automatically, eliminating the second manual Linux terminal.

A Ctrl+C test produced normal FFmpeg finalization output. Subsequent process inspection found no remaining `session-runtime`, `mame`, or `ffmpeg` process.

The session FIFOs remained in `/tmp/4play/session-4`. Removing stale session resources remains a cleanup task.

## Virtual-controller validation

Development access to `/dev/uinput` is provided by:

```udev
KERNEL=="uinput", GROUP="input", MODE="0660"
```

The `tools/uinput-test` crate creates `4-Play Virtual Controller` with:

- X and Y absolute axes
- four face buttons
- two shoulder buttons
- Select/Coin
- Start

Linux exposed the device through both an event handler and joystick handler. `jstest` reported two axes and eight buttons and showed generated axis and button changes.

The controller has not yet been integrated with the MAME session runtime.

## Current conclusions

Validated:

- centralized headless MAME is viable on the development host
- raw MAME media can bypass desktop capture
- Rust can bridge synchronized media in real time
- one encoder per session works for the current experiment
- simultaneous media sessions can remain independent
- CHD-backed games work through the same runtime shape
- the runtime can create a suitable Linux virtual controller

Not yet validated:

- MAME consuming the runtime-created controller
- remote seat input
- safe disconnect and input timeout
- button-to-photon latency
- long-duration soak behavior
- complete two-session controller and save isolation
- hardware capacity limits

## Next experiment

Integrate `VirtualController` into `session-runtime`, create it before launching MAME, and prove that generated Coin, Start, directional, and action-button states change the running game visible in the remote stream.

Only after that local path works should the project add seat-to-runtime controller-state networking.
