# WebRTC browser media spike

This runbook validates the browser-native media path without changing the
working UDP MPEG-TS/FFplay gameplay path.

The first spike uses:

```text
Linux GStreamer test pattern
  -> webrtcbin
  -> manual SDP answer
  -> browser /webrtc-test page
  -> in-page <video>
```

Manual SDP copy/paste is deliberate. It lets us prove codec compatibility,
browser playback, and latency behavior before adding production signaling to the
control plane.

## Safety guardrails

- Do not enable `media_transport: "web_rtc"` for normal clients yet.
- Do not remove or alter the UDP/FFplay session path.
- Test with a generated pattern first, not MAME media.
- Favor dropped frames over a growing buffer.
- If playback connects but delay grows over time, stop and investigate before
  integrating with session-runtime.

## Prerequisites

On the Linux runtime host:

```bash
cd ~/src/4-play
bash deploy/systemd/check-browser-media-prereqs.sh
```

The checker must report the GStreamer WebRTC elements and Python GI bindings as
present. If the Python GI check is missing on Debian, investigate:

```bash
sudo apt install -y \
  python3-gi \
  gir1.2-gstreamer-1.0 \
  gir1.2-gst-plugins-bad-1.0
```

## Start the browser page

From Windows, open:

```text
http://192.168.20.68:8080/webrtc-test
```

Then click:

1. `Create local peer`
2. `Create SDP offer`
3. `Copy offer`

## Send the offer to the Linux publisher

In SSH on the Linux host, paste the offer into a file:

```bash
cat > /tmp/4play-webrtc-offer.sdp <<'EOF'
PASTE_THE_BROWSER_OFFER_HERE
EOF
```

Start the test-pattern publisher:

```bash
cd ~/src/4-play
python3 tools/webrtc-gst-test-pattern.py \
  --offer-file /tmp/4play-webrtc-offer.sdp \
  --answer-file /tmp/4play-webrtc-answer.sdp
```

Leave this command running. It owns the live GStreamer publisher.

## Apply the answer in the browser

In another SSH terminal, print the answer:

```bash
cat /tmp/4play-webrtc-answer.sdp
```

Paste the full SDP answer into the browser page's `Remote answer` box and click:

```text
Apply answer
```

Expected first-pass result:

- the video surface starts showing a GStreamer test pattern;
- connection state moves toward `connected`;
- receiver stats begin increasing;
- decoded frames increase;
- dropped frames may increase occasionally, but playback should not drift into a
  larger and larger delay.

## Stop the test

Press `Ctrl-C` in the terminal running:

```text
tools/webrtc-gst-test-pattern.py
```

Then click `Reset` in the browser.

## What to record

Record:

- browser used;
- whether video appears;
- whether audio ticks are heard;
- connection state;
- decoded frames;
- dropped frames;
- packet loss;
- jitter;
- whether visible delay grows over 5-10 minutes;
- CPU load on the Linux host.

## Success criteria for this spike

This spike is successful when:

- the browser renders live video in the page;
- audio and video remain synchronized enough for a generated pattern;
- the stats counters do not suggest growing buffering;
- the Linux host remains stable;
- the UDP/FFplay gameplay path remains untouched.

Only after this passes should we build the next slice: control-plane signaling
and a session-runtime publisher tied to real MAME media.
