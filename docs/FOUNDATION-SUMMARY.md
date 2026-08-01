# Foundation Summary

The 4-Play foundation now includes both the original architectural boundaries and a validated first runtime implementation.

## Product and architecture decisions

- 4-Play uses a monorepository.
- Seats are thin clients and do not own authoritative emulator state.
- The future control plane coordinates identity, catalog, sessions, player slots, and host selection.
- Controller input, video, and audio travel directly between seats and assigned runtime hosts when practical.
- Phase 1 may run the control plane and runtime host on the existing Linux server.
- Emulator behavior is isolated behind runtime adapters and typed game metadata.
- Player positions are modeled explicitly because cabinet position may affect character, team, or controls.

## Runtime evidence now established

The Rust `session-runtime` can:

- prepare isolated session directories and media FIFOs
- launch and own one headless MAME process
- launch and own one FFmpeg encoder process
- read MAME raw video and raw PCM audio concurrently
- maintain small bounded media queues
- stream H.264/AAC as MPEG-TS over UDP to a remote seat
- shut down without leaving MAME or FFmpeg orphan processes in the validated Ctrl+C test

The media pipeline has been validated with:

- Aliens at 288×224 and 59.185606 Hz
- TMNT at 320×224 and 60.000000 Hz
- Killer Instinct at 320×240 and 58.981183 Hz
- a CHD-backed game layout
- two simultaneous independent MAME sessions

A separate Rust `uinput-test` creates a Linux virtual controller with two axes and eight buttons. Linux exposes it as both an event device and joystick device, and `jstest` confirms its generated input state.

## Current boundary

The media and process-lifecycle foundation is working. Remote play is not yet complete because the virtual controller has not been integrated into `session-runtime`, no seat input transport exists yet, and button-to-photon latency has not been measured.

## Next proof

The next narrow proof is:

```text
runtime-owned virtual controller
  → launched MAME session
  → visible game response in the remote media stream
```

After that works locally, the project can add seat-to-runtime controller-state transport and perform the Phase 1A latency and disconnect tests.
