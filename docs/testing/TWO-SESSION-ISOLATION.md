# Two-Session Isolation Test

## Purpose

This test separates four different claims that are easy to conflate:

1. two sessions can run concurrently
2. each session owns distinct media, process, port, and virtual-input resources
3. input sent to one runtime reaches only that runtime's virtual controller
4. each MAME process consumes only its intended virtual controller

The automated harness proves the first three claims and independent shutdown.
The short manual dual-window procedure is required for the fourth claim because
observing an event on the correct Linux input device does not prove that another
MAME process has not also opened that device.

Save and NVRAM *path* separation is visible during this test. Save-content
isolation is a separate follow-up because it requires games that reliably write
identifiable persistent data.

## Automated test

### Coverage

`tools/two-session-isolation.sh`:

- launches TMNT and Aliens concurrently
- assigns different session IDs, working directories, stream ports, and input
  ports
- receives and decodes both MPEG-TS streams on separate localhost ports
- verifies the streams have their expected, different resolutions
- identifies the separate `/dev/input/event*` devices created by each runtime
- injects a distinct button/direction pattern into each UDP input port
- verifies each pattern appears only on the expected virtual input device
- stops session A and verifies its MAME, FFmpeg, and virtual controller are gone
- verifies session B continues producing video and accepting input
- stops session B and verifies its resources are cleaned up
- writes every assertion and supporting log into a timestamped result directory

### Prerequisites

- `/dev/uinput` is writable by the test user
- the test user can read `/dev/input/event*`
- `evtest`, FFmpeg, and the Rust toolchain are installed
- the TMNT and Aliens ROMs are available to the configured MAME installation
- UDP ports 41301, 41302, 42301, and 42302 are unused

### Run

From the repository on the Linux runtime host:

```bash
source /home/blake/.cargo/env
cd ~/src/4-play
cargo build -p session-runtime -p input-inject
./tools/two-session-isolation.sh
```

The optional first argument controls how many seconds session B must continue
after session A stops. The optional second argument sets the result directory:

```bash
./tools/two-session-isolation.sh 30 /tmp/4play-two-session-validation
```

Success ends with:

```text
OVERALL PASS
```

Any failed assertion makes the script exit nonzero. The script tracks exact
PIDs and attempts to clean up both sessions and both local receivers after
success, failure, interruption, or termination. It intentionally retains the
session directories and test logs for inspection.

### Automated validation record

The harness passed on the reference Linux host on 2026-10-04:

- TMNT and Aliens produced and decoded separate 320x224 and 288x224 streams
- the runtimes created `/dev/input/event21` and `/dev/input/event22`
- action 1 plus left reached only session A's device
- action 6 plus right reached only session B's device
- session A exited with status 0 and reaped its MAME and FFmpeg children
- session A's virtual controller disappeared while session B's remained
- session B advanced from 274 to 925 video frames after session A stopped
- session B continued to accept a second action/direction pattern
- session B then exited with status 0 and reaped its children
- the final process check found no remaining runtime, MAME, FFmpeg, or evtest
  process

All automated assertions passed. This establishes resource and runtime-input
routing isolation, but not yet MAME-level input consumption isolation.

### Per-session MAME controller assignment

The first manual dual-window attempt found that neither game responded to its
seat client. Live process inspection showed that TMNT had opened the first
virtual controller while Killer Instinct had opened both virtual controllers.
The runtimes used new per-session MAME `cfg` directories, but neither MAME
process loaded the previously created `4play.cfg` controller profile. Both
virtual controllers also exposed the same Linux input identity, so merely
enabling that shared profile would not have selected the correct device.

The runtime now:

- gives every session/player controller a unique USB vendor/product/version
  identity derived from the full session ID and player number
- gives the controller a session-specific display name
- generates `ctrlr/4play-session.cfg` inside the session directory
- maps only that controller's stable device-ID substring to `JOYCODE_1`
- supplies explicit direction, six-action, coin, and start mappings
- launches MAME with the session's `-ctrlrpath` and `-ctrlr 4play-session`
- enables joystick input with MAME's `sdljoy` provider so dynamically created
  uinput devices are enumerated even though they are not present in SDL's
  game-controller mapping database

The strengthened automated harness passed all 49 checks on 2026-10-04 using
commit `33e407d`. It verified that the two generated profiles contained
different device IDs, both MAME commands explicitly selected their own
profiles and the `sdljoy` provider, controller events remained isolated, and
session B advanced from 268 to 623 frames after session A stopped.

## Manual MAME-level input isolation

The automated result must be followed by this visual test. Use two SSH windows
on the Linux host and four PowerShell windows on the Windows seat.

The runtime host firewall must allow input UDP from the seat. The reference
environment uses this narrowly scoped UFW rule:

```bash
sudo ufw allow from 192.168.20.10 to any port 42011:42012 proto udp \
  comment '4-play two-session input'
```

Without this rule, the input clients continue running and sending heartbeats,
but neither runtime logs `Seat input connected` and no controls register.

### 1. Start both Windows receivers

Receiver A:

```powershell
.\ffplay.exe -f mpegts `
  -fflags nobuffer -flags low_delay -framedrop `
  -probesize 32768 -analyzeduration 0 `
  "udp://0.0.0.0:41011?fifo_size=1000000&overrun_nonfatal=1"
```

Receiver B:

```powershell
.\ffplay.exe -f mpegts `
  -fflags nobuffer -flags low_delay -framedrop `
  -probesize 32768 -analyzeduration 0 `
  "udp://0.0.0.0:41012?fifo_size=1000000&overrun_nonfatal=1"
```

### 2. Start both Linux sessions

Session A (TMNT):

```bash
cd ~/src/4-play
./target/release/session-runtime \
  --session-id 11 \
  --rom tmnt \
  --width 320 \
  --height 224 \
  --fps 60 \
  --destination-ip 192.168.20.10 \
  --udp-port 41011 \
  --input-port 42011
```

Session B (Killer Instinct):

```bash
cd ~/src/4-play
./target/release/session-runtime \
  --session-id 12 \
  --rom kinst \
  --width 320 \
  --height 240 \
  --fps 58.981183 \
  --destination-ip 192.168.20.10 \
  --udp-port 41012 \
  --input-port 42012
```

### 3. Start both Windows input clients

Input A:

```powershell
cargo run -p seat-input -- 192.168.20.68:42011
```

Input B:

```powershell
cargo run -p seat-input -- 192.168.20.68:42012
```

Only the focused PowerShell window receives keyboard events, so test one seat
at a time:

1. Focus Input A. Insert a coin, start TMNT, move, and attack. Confirm KI never
   responds.
2. Focus Input B. Insert a coin, start KI, move, and use several attacks.
   Confirm TMNT never responds.
3. Alternate between inputs several times and test held directions plus
   simultaneous movement/actions.
4. Press `Esc` in Input A. Confirm session A closes while session B continues
   streaming and accepting input for at least five minutes.
5. Press `Esc` in Input B and confirm it also shuts down cleanly.

Record this manual matrix:

| Assertion | Result | Notes |
| --- | --- | --- |
| Input A affects TMNT | PASS | User confirmed visible movement and attacks. |
| Input A does not affect KI | PASS | User confirmed the two inputs remained isolated. |
| Input B affects KI | PASS | User confirmed movement and all six attack controls. |
| Input B does not affect TMNT | PASS | User confirmed the two inputs remained isolated. |
| Simultaneous controls work in A | PENDING | The attempted run was interrupted before a conclusive result. |
| Simultaneous controls work in B | PENDING | The attempted run was interrupted before a conclusive result. |
| B remains playable after A stops | PASS | KI's runtime, MAME, encoder, receiver, and input client survived a complete TMNT stop/restart. |
| Both sessions clean up | PARTIAL | Both runtimes, MAME children, server FFmpeg encoders, input clients, and input ports cleaned up. The standalone Windows FFplay receivers required manual termination. |

During the 2026-10-04 manual run, TMNT was stopped and relaunched using the
same session ID and ports. KI continued playing without a process restart, and
the relaunched TMNT input client reconnected successfully. This confirms the
manual session-restart isolation path in addition to the automated shutdown
test.

Pressing `Esc` in each input client sends the runtime stop flag and cleanly
stops the corresponding Linux runtime, MAME process, and FFmpeg encoder. It
does not stop the independently launched Windows FFplay receiver. MPEG-TS over
UDP has no connection close or end-of-stream signal, so receiver lifecycle
management remains a seat-orchestration task. The two receivers were closed
manually at the end of this test.

If either game responds to the other session's input, the test has found a real
Phase 1B isolation failure. Do not reinterpret it as a test-harness problem:
the next implementation task would be restricting each MAME process to its own
virtual input device.
