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

On 2026-10-03, an isolated cleanup test recorded the exact runtime, FFmpeg, and
MAME PIDs, sent SIGTERM only to the Rust runtime, and waited for shutdown. The
runtime exited with status 0 and all three PIDs were absent afterward. The
runtime now converts SIGINT and SIGTERM into an orderly shutdown request and
also keeps kill-and-reap guards around both child processes for error paths.

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

At the time of the initial validation, the controller had not yet been
integrated with the MAME session runtime.

### SSH terminal input experiment

The current development build adds an opt-in `--terminal-input` mode to
`session-runtime`. The runtime creates the virtual controller before MAME starts
and reads individual characters from the controlling SSH terminal in raw mode.

The development key layout is:

| Terminal key | Controller input |
| --- | --- |
| `W`, `A`, `S`, `D` | up, left, down, right |
| `J`, `K`, `L`, `;` | action buttons 1 through 4 |
| `U`, `I` | action buttons 5 and 6 |
| `1` | coin/select |
| `2` | player start |
| `Q` | stop MAME and end the session |

Traditional SSH terminals do not transmit key-release events. Each received
character therefore produces a 120 ms press followed by an explicit release.
This mode is intended to validate playable terminal-to-MAME input, not to serve
as the production seat input transport.

Example for TMNT on the reference network:

```bash
cargo run -p session-runtime -- \
  --session-id 5 \
  --rom tmnt \
  --width 320 \
  --height 224 \
  --fps 60 \
  --destination-ip 192.168.20.10 \
  --udp-port 41005 \
  --terminal-input
```

The terminal is restored and all virtual controls are neutralized when input
mode ends. A terminal read failure also terminates the owned MAME process.

### State-based seat input experiment

The `seat-input` client replaces terminal-character taps with real key-down and
key-up state captured on Windows. It sends a versioned controller snapshot
immediately after each transition and every 50 ms as a heartbeat. One snapshot
contains both axes and the complete button bitset, so diagonals, movement plus
an action, and multi-button combinations remain simultaneous.

Start the runtime receiver on Linux:

```bash
cargo run -p session-runtime -- \
  --session-id 6 \
  --rom tmnt \
  --width 320 \
  --height 224 \
  --fps 60 \
  --destination-ip 192.168.20.10 \
  --udp-port 41006 \
  --input-port 42000
```

Then start the input client from the repository on Windows:

```powershell
cargo run -p seat-input -- 192.168.20.68:42000
```

The runtime accepts one UDP source for this development session, rejects stale
sequence numbers, and neutralizes the controller after 250 ms without a valid
packet. After timeout, a restarted client may reconnect from a new source port.
This experiment does not yet authenticate or encrypt input packets.

Repeated play tests confirmed that the state-based path supports diagonals,
movement plus actions, and multi-button combinations without stuck controls.
With low-buffer FFplay, control-to-video response was subjectively excellent.
The remaining noticeable issue was audio occurring after the corresponding
visible action. A `-sync video` receiver experiment delayed all presentation and
was rejected; the video path must remain latency-first while audio is tuned.

To reduce player-side buffering while evaluating input latency, use a low-cache
receiver. For VLC:

```powershell
vlc --network-caching=50 "udp://@:41006"
```

Or, when FFplay is available:

```powershell
ffplay -fflags nobuffer -flags low_delay -framedrop -probesize 32 -analyzeduration 0 "udp://@:41006"
```

The runtime now supports an audio-only experiment switch while leaving the
validated video path unchanged:

```text
--audio-codec aac|opus
--audio-block-ms 1..100
```

Defaults remain AAC and 20 ms. The low-delay comparison profile uses Opus with
5 ms frames and 5 ms PCM bridge blocks:

```text
--audio-codec opus --audio-block-ms 5
```

Once per second the runtime reports first audio/video arrival, their startup
offset, queue depth, and dropped media counts.

The FFmpeg raw-audio input queue is independently configurable:

```text
--audio-thread-queue-size 1..1024
```

The historical default is 64 packets. Because the synchronized FFmpeg
flash/beep test remained precisely aligned through the downstream delivery
path, the first game test should reduce only this queue to 4 packets while
retaining AAC and 20 ms bridge blocks.

## Synthetic downstream A/V validation

On October 3, 2026, FFmpeg's synchronized flash/beep source was encoded as
H.264/AAC, muxed as MPEG-TS, sent over the wired LAN, and rendered by FFplay on
Windows through the normal audio output. The visible marker and beep were
perceptually simultaneous.

This result indicates that the large game-only action-to-sound delay is not
inherent to AAC, MPEG-TS, the network, FFplay, Windows mixing, or the HDMI
output. The remaining investigation boundary is MAME timing and the raw
audio/FIFO/bridge/input-queue path used only by game sessions.

## Current conclusions

Validated:

- centralized headless MAME is viable on the development host
- raw MAME media can bypass desktop capture
- Rust can bridge synchronized media in real time
- one encoder per session works for the current experiment
- simultaneous media sessions can remain independent
- CHD-backed games work through the same runtime shape
- the runtime can create a suitable Linux virtual controller
- MAME consumes runtime-owned controller state
- remote state-based input supports simultaneous controls
- timeout neutralization and reconnect behavior work
- low-buffer FFplay provides subjectively responsive input-to-video play

Not yet validated:

- button-to-photon latency
- objective action-to-sound delay and the best audio profile
- long-duration soak behavior
- complete two-session controller and save isolation
- hardware capacity limits

## Next experiment

Measure the responsive baseline objectively, then compare AAC/20 ms,
AAC/5 ms, and low-delay Opus/5 ms while holding all video and receiver settings
constant.
