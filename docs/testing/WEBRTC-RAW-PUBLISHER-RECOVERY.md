# Raw MAME WebRTC publisher: encoder failure regression

The reference host connected successfully to the browser on 2026-10-12, then
stopped with `vp8enc0: Failed to encode frame: invalid parameter`. This establishes
an encoder failure after ICE connected; it does not establish a network failure.

The old pipeline timestamped FIFO byte chunks with `filesrc do-timestamp=true`
before `rawvideoparse` assembled complete frames. Inconsistent frame timestamps
are the suspected encoder failure cause. The replacement assembles full BGRx
frames and 20 ms PCM blocks, feeds live `appsrc` elements, and assigns strictly
increasing timestamps from one shared monotonic clock. Queues are bounded and
drop old buffers. Input readers stay open and drain the FIFOs across browser
reconnects, preserving frame boundaries and avoiding stale input accumulation.

## Automated verification

```bash
cd ~/src/4-play
python3 tools/test-webrtc-raw-publisher.py
```

Framing tests exercise partial reads, a temporarily absent writer, and shared
audio/video timing. On Linux with GStreamer bindings the encoder test exercises
the actual VP8/Opus branches, including bursts and rebuilding the pipeline.
Windows skips the real encoder test because those bindings are unavailable.
This test does not establish browser playback or end-to-end latency.

## Deploy and manually validate

```bash
cd ~/src/4-play
git pull --ff-only origin main
sudo systemctl restart 4play-runtime-host-agent.service
```

Restarting the host agent may affect active games. End testing sessions first.
No Rust rebuild is required for this Python-only publisher change. Start a fresh
MAME session from the client, connect browser media, and verify that gameplay
continues past the startup pattern. Check sound synchronization, held movement,
and playback over at least five minutes. Refresh and reconnect to verify recovery.

To inspect the newest publisher log without a placeholder session ID:

```bash
find /tmp/4play/host-agent -maxdepth 1 -name '*.webrtc.log' -printf '%T@ %p\n' |
  sort -nr | head -1 | cut -d' ' -f2- | xargs -r tail -250
```

If it fails, retain the encoder/pipeline error and the last WebRTC connection
state. Do not mark browser playback validated based solely on encoder tests.
