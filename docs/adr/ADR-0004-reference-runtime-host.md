# ADR-0004: Reference Runtime Host

**Status:** Accepted

**Date:** 2026-07-27

## Context

4-Play requires a reference runtime environment to develop, benchmark, and validate the platform before scaling to multiple runtime hosts.

The reference runtime host must be capable of:

- Running MAME with low CPU overhead
- Supporting Intel Quick Sync hardware video encoding
- Hosting the Runtime Manager
- Managing game sessions
- Streaming gameplay to remote clients
- Running alongside existing self-hosted applications

Rather than designing around hypothetical hardware, the project will use Blake's existing Linux server as the baseline reference platform.

---

# Reference Hardware

| Component | Value |
|-----------|------|
| Model | Lenovo ThinkCentre M910 Tower |
| CPU | Intel Core i5-7600 |
| Cores | 4 |
| Base Clock | 3.50 GHz |
| Memory | 16 GB DDR4 |
| Storage | Samsung PM981 NVMe SSD (512 GB) |
| Graphics | Intel HD Graphics 630 |
| Network | Intel I219-LM Gigabit Ethernet |
| Link Speed | 1000 Mbps Full Duplex |

---

# Operating System

| Component | Value |
|-----------|------|
| Distribution | Debian GNU/Linux 13 (Trixie) |
| Kernel | Linux 6.x |
| SSH | Enabled (Key Authentication) |
| Firewall | UFW |

---

# Runtime Layout

```
/opt/4play
├── config
│   └── mame
├── library
│   └── roms
├── logs
├── media
├── runtimes
├── sessions
│   ├── snapshots
│   └── state
└── tools
```

---

# MAME

Version:

```
0.276
```

Installed From:

```
Debian Package Repository
```

Executable:

```
/usr/games/mame
```

Configuration:

```
/opt/4play/config/mame/mame.ini
```

ROM Library:

```
/opt/4play/library/roms
```

---

# Video Encoding

Intel Quick Sync hardware acceleration has been verified.

Available hardware encoders include:

- h264_qsv
- h264_vaapi
- hevc_qsv
- hevc_vaapi
- mjpeg_vaapi

Hardware encoding successfully completed using:

- VAAPI
- H.264
- 640x480
- 60 FPS

Performance:

- 10 second encode
- Completed at approximately **34× realtime**

---

# Networking

Host Network:

```
Homelab VLAN
```

Network Interface:

```
Intel I219-LM
```

Negotiated Link:

```
1000 Mbps
Full Duplex
Auto Negotiation
```

---

# Runtime Validation

Validated:

- Debian 13
- MAME 0.276
- TMNT (World 4 Players)
- Aliens
- Intel Quick Sync
- VAAPI
- Gigabit Networking
- Managed configuration directory

---

# Local Performance Baseline

Test Game:

```
Teenage Mutant Ninja Turtles
(World 4 Players)
```

Observed Memory:

```
~310 MB Resident
```

Observed CPU:

```
~25% process utilization
```

Overall System Utilization:

| Metric | Value |
|---------|------:|
| CPU Idle | ~90% |
| User CPU | ~7% |
| System CPU | ~1% |
| Swap | 0 |
| Available RAM | ~14 GB |

The runtime host remains largely idle while executing a local MAME session.

---

# Important Discovery

Launching SDL-based MAME directly from a Linux text console (TTY) does not properly receive keyboard input.

Launching MAME within an X session functions correctly.

The runtime architecture should therefore assume a lightweight graphical session rather than direct console execution.

Future runtime management may replace this with a virtual X server or another headless display solution suitable for streaming.

---

# Architectural Implications

The Runtime Host is responsible only for executing emulator sessions.

Future runtime flow:

```
Game Package
        │
        ▼
Runtime Manager
        │
        ▼
Launch Manifest
        │
        ▼
Virtual Display
        │
        ▼
SDL
        │
        ▼
MAME
        │
        ▼
Frame Capture
        │
        ▼
Intel Quick Sync
        │
        ▼
Network Stream
        │
        ▼
Seat Client
```

The Runtime Manager owns emulator lifecycle, session management, logging, save-state directories, controller mapping, and stream creation.

MAME is treated as an implementation detail rather than the core runtime.

---

# Decision

The Lenovo ThinkCentre M910 running Debian 13 is accepted as the official Phase 1 reference runtime host.

Future runtime hosts should meet or exceed the capabilities documented here or provide justification for architectural deviations.

---

# Future Work

- Benchmark hardware encoding latency
- Benchmark end-to-end stream latency
- Validate multiple simultaneous sessions
- Evaluate virtual display solutions
- Measure network bandwidth during gameplay
- Compare VAAPI vs Quick Sync performance
- Establish runtime monitoring and diagnostics