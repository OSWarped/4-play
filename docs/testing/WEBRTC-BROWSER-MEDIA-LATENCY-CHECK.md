# WebRTC browser media latency check

Use this checklist after the video-only WebRTC spike connects and shows the
moving test pattern.

## Start video-only baseline

Linux:

```bash
cd ~/src/4-play
python3 tools/webrtc-gst-test-pattern.py \
  --video-only \
  --http-signal 0.0.0.0:18080
```

Browser:

```text
http://192.168.20.68:8080/webrtc-test
```

Use:

1. `Reset`
2. unchecked `Include audio in this spike offer`
3. `Create local peer`
4. `Create SDP offer`
5. `Signal publisher`

## Observe for 10 minutes

Record at start, 1 minute, 5 minutes, and 10 minutes:

| Time | ICE | Peer | Remote tracks | Frames decoded | Frames dropped | Packets lost | Jitter seconds | RTT seconds | Bytes received | Visible delay growing? |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Start | | | | | | | | | | |
| 1 min | | | | | | | | | | |
| 5 min | | | | | | | | | | |
| 10 min | | | | | | | | | | |

Pass criteria:

- ICE remains connected/completed.
- Peer remains connected.
- Frames decoded continues increasing.
- Occasional dropped frames are acceptable.
- Delay does not visibly grow over time.
- Browser video does not freeze.
- Linux publisher does not crash.

Fail criteria:

- ICE disconnects/fails.
- Frames decoded stops increasing.
- Delay grows steadily.
- Browser video freezes while publisher is still running.
- Linux CPU saturates enough to threaten session-runtime headroom.

## Linux host observation

In a second SSH terminal:

```bash
top -p "$(pgrep -f webrtc-gst-test-pattern.py | head -1)"
```

Optional one-line snapshot:

```bash
ps -o pid,pcpu,pmem,etime,args -p "$(pgrep -f webrtc-gst-test-pattern.py | head -1)"
```

## Audio follow-up

After video-only passes, restart the publisher without `--video-only`:

```bash
python3 tools/webrtc-gst-test-pattern.py \
  --http-signal 0.0.0.0:18080 \
  --answer-file /tmp/4play-webrtc-answer.sdp
```

In the browser:

1. check `Include audio in this spike offer`
2. `Reset`
3. `Create local peer`
4. `Create SDP offer`
5. `Signal publisher`

Expected:

- video renders;
- audio tick/beep is audible;
- remote track count reaches 2;
- audio samples decoded increases in the receiver stats;
- audio/video remain plausibly synchronized;
- no growing delay.

If audio causes negotiation or playback failure, inspect the publisher answer:

```bash
grep -n 'm=audio\|a=sendonly\|a=inactive\|rtpmap:111' /tmp/4play-webrtc-answer.sdp
```

The expected successful audio section includes `a=rtpmap:111 OPUS/48000/2` and
`a=sendonly`. If it instead shows `a=inactive`, reset the browser test page,
make sure `Include audio in this spike offer` is checked before creating the
local peer, then create a fresh offer.
