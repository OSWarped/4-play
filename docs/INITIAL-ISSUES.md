# Recommended GitHub Issues

This list now distinguishes completed feasibility work, active Phase 1A work, and later platform work.

## Completed or substantially demonstrated

1. **Prototype low-latency video and audio transport**
   - raw video and PCM audio emitted directly by MAME
   - concurrent Rust media bridge
   - bounded queues
   - FFmpeg H.264/AAC encoding
   - UDP MPEG-TS delivery to a Windows seat
   - synchronized playback across several games

2. **Implement the first MAME session runtime**
   - one-command session launch
   - per-session directories and FIFOs
   - MAME and FFmpeg process ownership
   - clean validated Ctrl+C termination

3. **Prove media-side concurrent-session isolation**
   - two simultaneous MAME instances
   - separate directories, FIFOs, bridges, encoders, and destination ports
   - independent game resolutions and refresh rates

4. **Create and validate a Linux virtual controller**
   - `/dev/uinput` access configured for development
   - two axes and eight buttons
   - Linux event and joystick handlers
   - generated states verified with `jstest`

These demonstrations do not close the broader remote-play or isolation milestones because controller integration and latency measurements remain.

## Active Phase 1A issues

1. **Integrate the virtual controller into `session-runtime`**
   - create the device before MAME launches
   - expose an API for directions, buttons, start, coin, and neutral state
   - prove MAME discovers and consumes the device

2. **Prototype direct seat-to-runtime input transport**
   - transmit complete controller-state snapshots
   - include sequence and session identity
   - measure update rate, loss, ordering, and jitter
   - neutralize input after timeout or disconnect

3. **Measure local and remote MAME latency**
   - record a local baseline
   - measure remote button-to-photon latency
   - report median, 95th-percentile, and 99th-percentile results
   - separate encode, network, decode, and display contributions where practical

4. **Complete the reference-environment inventory**
   - hardware and operating system
   - wired path and negotiated link speeds
   - baseline and session resource use
   - storage and encoder capabilities

5. **Document the Phase 1A decision**
   - summarize measured evidence
   - decide whether the current direct seat-to-runtime approach advances
   - identify the next bottleneck and accepted limitations

## Runtime hardening and Phase 1B

6. **Complete session lifecycle states**
   - use the existing state enum
   - publish meaningful launch, running, stopping, stopped, and failure state
   - remove obsolete dead-code warnings by wiring or deleting scaffolding

7. **Add automatic MAME metadata discovery**
   - resolve width, height, rotation, and refresh rate from MAME metadata
   - reject ambiguous multi-screen configurations
   - remove manual media dimensions from normal session requests

8. **Prove complete two-session isolation**
   - separate virtual controllers
   - no input leakage
   - independent saves and NVRAM
   - stop one session without disturbing the other

9. **Implement robust cleanup and recovery**
   - remove stale FIFOs and session resources
   - handle emulator and encoder termination
   - make start and stop behavior repeatable

## Phase 1C platform work

10. **Bootstrap the control-plane service**
    - health endpoint
    - configuration
    - structured logging
    - local persistence

11. **Implement runtime-host registration and heartbeat**
    - register capabilities
    - publish health
    - detect host loss

12. **Define the versioned session protocol**
    - request, allocation, launch, ready, active, stop, and failure messages
    - media and input endpoint grants

13. **Build the minimal seat client**
    - request a session
    - receive media
    - send controls
    - recover to a usable state

## Phase 2 preparation

14. **Design the active-session preview strategy**
    - measure preview approaches without adding one full encoder per viewer

15. **Implement player-slot reservation leases**
    - atomic reservation
    - reconnect grace
    - release and expiry

16. **Model fixed-character and positioned cabinets**
    - validate the slot model using representative arcade games
