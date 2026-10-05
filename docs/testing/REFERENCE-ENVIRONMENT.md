# Reference Environment

## Status

Initial development environment recorded. Hardware-capacity and utilization measurements remain incomplete.

## Runtime host

- Host name: `blake-web`
- Operating system: Linux
- Runtime address: `192.168.20.68`
- Source repository: `~/src/4-play`
- Custom MAME source/build: `~/src/mame-4play`
- MAME configuration path: `/opt/4play/config/mame`
- Canonical ROM library path: `/opt/4play/library/roms`
- Session working root: `/tmp/4play`
- Network interface observed during packet capture: `enp0s31f6`

## Development seat

- Operating system: Windows
- Address: `192.168.20.10`
- Media receiver used for validation: VLC
- Connection: wired Ethernet on the `192.168.20.0/24` network

## Software

- Rust workspace with `session-runtime` and `uinput-test`
- FFmpeg 7.1.5 from Debian packages
- libx264 enabled
- AAC encoder enabled
- Linux `uinput` module enabled
- `/dev/uinput` development permissions: root owner, `input` group, mode `0660`

## Validated network behavior

A packet capture on the Linux host confirmed outbound UDP traffic from `192.168.20.68` to the Windows seat. The active media transport is unicast UDP MPEG-TS. High-numbered ports are used for development after discovering a Windows system-service conflict on UDP port 5004.

## Runtime host firewall requirements

For a Windows seat at `192.168.20.10` connecting to the reference Linux runtime
host at `192.168.20.68`, UFW must allow:

- TCP `8080` from the seat for the control plane
- UDP `41000:41099` from the seat for media sessions
- UDP `42000:42099` from the seat for seat input

Reference UFW setup:

```bash
sudo ufw allow from 192.168.20.10 to any port 8080 proto tcp \
  comment '4-play control plane'
sudo ufw allow from 192.168.20.10 to any port 41000:41099 proto udp \
  comment '4-play media range'
sudo ufw allow from 192.168.20.10 to any port 42000:42099 proto udp \
  comment '4-play seat input range'
```

The input range is required for multi-seat play. Seat 1 uses `42000`, seat 2
uses `42001`, seat 3 uses `42002`, and later seats continue upward. If only
`42000/udp` is allowed, seat 1 can work while later seats fail. In that failure
mode `tcpdump` on the host can still show valid UDP packets arriving at the NIC,
but `session-runtime` will not receive them or log `Seat input connected`
because UFW drops the packets before socket delivery.

## Existing host workloads

The Linux server also hosts other personal applications. Their exact baseline CPU, memory, disk, and network usage has not yet been recorded for this experiment.

## Missing inventory

The following items still need to be collected before Issue #2 is complete:

- CPU model, core count, thread count, and virtualization support
- total RAM and baseline RAM use
- GPU model, driver, render-node permissions, and hardware encoding support
- storage devices, filesystem, free capacity, and measured I/O behavior
- negotiated Ethernet link speed on host and seat
- baseline CPU, RAM, disk, GPU, and network utilization
- per-session resource use for MAME, Rust, FFmpeg, and the seat decoder
- impact of existing trivia, portfolio, and other workloads

## Current conclusion

The host is suitable for continued feasibility development. No conclusion has yet been made about production capacity or the maximum sustainable number of simultaneous sessions.
