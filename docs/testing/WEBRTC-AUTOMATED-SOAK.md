# Automated WebRTC stability and resource soak

## Purpose and boundaries

Use this Linux-only fixture before changing the receiver cap or media pipeline.
It uses generated 304x224, 60 Hz raw video and silent 48 kHz audio, temporary files,
local GStreamer receivers, and the test ICE pool `51000-51099`. It does not call
the control plane, start MAME, read tokens, change firewall rules, or stop services.
Do not run many copies at once: encoding and software decoding consume CPU.

The fixture first checks two players and a spectator, spectator rejection of input,
promotion to P3 at capacity, isolated input, P2 reconnect and native ICE disposal.
The optional steady-state soak then runs **three players**, renews their test input
leases, checks authenticated input for each, and periodically replaces P2. P1/P3
must continue decoding; every sampled receiver must advance both video and audio.
The normal short run retains the malformed-offer isolation regression check;
the soak mode finishes after its own receiver cleanup instead.

Measurements cover **the generated publisher plus all three software decoding
clients in one process**. CPU 100% means one CPU core, not the entire machine.
Audio block rate is a decoded-buffer count, not samples/second or sync error.
RSS variation is observational and can reflect allocator caches; this fixture
does not prove a leak or latency growth from RSS alone. It does not exercise
the actual MAME FIFO feeder, music, browser jitter buffers, network routing, or
input-to-photon timing. Keep WebRTC opt-in and FFplay available until those
gameplay gates are independently accepted.

## Run on Linux

From the repository root, with installed GStreamer/Python bindings:

```bash
python3 tools/test-webrtc-soak-metrics.py
G_DEBUG=fatal-criticals python3 tools/test-webrtc-multi-peer.py
```

For a ten-minute soak, logging to a uniquely created temporary directory:

```bash
results_dir="$(mktemp -d /tmp/4play-webrtc-soak-results.XXXXXX)"
set -o pipefail
G_DEBUG=fatal-criticals python3 -u tools/test-webrtc-multi-peer.py \
  --soak-seconds 600 --sample-seconds 5 --reconnect-seconds 30 \
  2>&1 | tee "$results_dir/soak.log"
printf 'Results: %s\n' "$results_dir/soak.log"
```

`pipefail` preserves a failing test exit status when piping through `tee`.
The maximum duration is 24 hours. `--reconnect-seconds 0` disables reconnect churn.
Optional `--max-rss-mib N` fails if the **combined fixture** exceeds a chosen budget;
default zero only observes. Sampling is 1-60 seconds, default five. Startup and
reconnect negotiation and a final complete sample may extend wall-clock runtime
beyond the requested soak (the final sample adds at most one sampling interval).
Ctrl+C runs normal fixture cleanup; no production services are involved.

Lines beginning `SOAK ` contain JSON start/sample/passed records with decoded
rates, process CPU, RSS, descriptors, threads, reconnect count and peak resources.
Logs never include test input tokens. A nonzero exit, absent final `passed` record,
stalled decoder, incorrect input, extra peer, or failed lease renewal is failure.
Inspect the samples rather than inferring a plateau from one peak or end value.

## Remaining real-game validation

During longer real play, use the existing Phase 3 monitor and browser receiver
diagnostics. Record game, runtime profile, peer count, host resources, perceived
controls, music sync, fullscreen/menu behavior, and reconnect isolation. The
operator should choose a practical party-session duration. Synthetic decoding
does not replace this acceptance or authorize increasing the eight-peer cap.

## Initial reference-host result — 2026-10-14

A 180-second run with five-second samples and P2 reconnect every 30 seconds
passed: 36 observations, five reconnects, approximately 60 decoded video frames
and 50 audio buffers per second per receiver, with all input checks passing.
Combined-process CPU stayed near 59-60% of one core. RSS rose from 93.9 to
109.7 MiB, descriptors rose from 159 to 179 then stayed at 179, and observed
threads peaked at 129. The run used generated low-complexity frames and silence.

This is a short stability baseline, **not a long-term memory plateau**: the RSS
increase needs a longer reconnect/no-reconnect comparison before attributing it
to allocator caching or leaks. The peer cap remains unchanged. The standard
short multi-peer regression passed separately; an intentionally impossible 1 MiB
combined RSS ceiling correctly failed. Portable metric/policy/address/pool tests
and all 22 browser input/media/spectator tests passed. No production services or
firewall settings were changed for these checks.
