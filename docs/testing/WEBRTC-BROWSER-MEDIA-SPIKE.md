# WebRTC browser media spike

This runbook validates the browser-native media path without changing the
working UDP MPEG-TS/FFplay gameplay path.

The first spike uses:

```text
Linux GStreamer test pattern
  -> webrtcbin
  -> small HTTP /offer endpoint
  -> browser /webrtc-test page
  -> in-page <video>
```

The spike HTTP endpoint is intentionally separate from the production control
plane. It automates the fragile copy/paste signaling loop while we prove codec
compatibility, browser playback, ICE behavior, and latency.

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

## Start the Linux publisher

In SSH on the Linux host, start the test-pattern publisher:

```bash
cd ~/src/4-play
python3 tools/webrtc-gst-test-pattern.py \
  --video-only \
  --http-signal 0.0.0.0:18080
```

Leave this command running. It owns the live GStreamer publisher and its
temporary `/offer` signaling endpoint.

## Optional: signal through the control plane

The preferred production-shaped spike path is:

```text
browser /webrtc-test
  -> authenticated control-plane experimental endpoint
  -> standalone local publisher on 127.0.0.1:18080
```

To enable that proxy, set this on the Linux control-plane service environment:

```bash
FOURPLAY_WEBRTC_TEST_PATTERN_PUBLISHER_URL=http://127.0.0.1:18080/offer
FOURPLAY_WEBRTC_BROWSER_HOST_IP=192.168.20.10
```

Then rebuild/restart the control plane:

```bash
cd ~/src/4-play
cargo build --release -p control-plane-server
sudo systemctl restart 4play-control-plane.service
```

The reference systemd deployment also includes an optional local publisher unit:

```bash
sudo deploy/systemd/install-reference-server.sh
sudo systemctl enable --now 4play-webrtc-spike-publisher.service
sudo systemctl restart 4play-control-plane.service
```

The runtime-host agent can also own the experimental publisher lifecycle. This
is closer to the roadmap direction than the separate side service:

```bash
sudo systemctl disable --now 4play-webrtc-spike-publisher.service
sudo sed -i 's|^FOURPLAY_WEBRTC_TEST_PATTERN_SUPERVISED=.*|FOURPLAY_WEBRTC_TEST_PATTERN_SUPERVISED=1|' /etc/4play/4play.env
sudo systemctl restart 4play-runtime-host-agent.service
sudo systemctl restart 4play-control-plane.service
```

Use one mode at a time. The standalone publisher service and host-agent
supervision both default to `127.0.0.1:18080`; running both will create a bind
conflict.

Leave `FOURPLAY_WEBRTC_SESSION_PUBLISHERS=0` for this spike. The generated
test-pattern publisher can be supervised without advertising `web_rtc` for
normal game sessions. That advertisement is reserved for the future
session-runtime media publisher.

With the proxy enabled, `/webrtc-test` defaults `Publisher URL` to:

```text
http://192.168.20.68:8080/api/v1/experimental/webrtc/test-pattern/offer
```

Enter the seat API token in the page before clicking `Signal publisher`.

The page also includes a `Control-plane WebRTC readiness` panel. Click
`Refresh status` after entering the seat token. Expected current reference
behavior:

- `Test-pattern proxy`: `configured` when
  `FOURPLAY_WEBRTC_TEST_PATTERN_PUBLISHER_URL` is set;
- `Session offer endpoint`: `available`;
- `Session publisher`: `not available yet`.
- `WebRTC runtime hosts`: `none`.

If the runtime host is configured to advertise WebRTC prerequisites before real
session offer routing exists, `WebRTC runtime hosts` may list the host while
`Session publisher` still says `not available yet`. That is expected until real
session-runtime media is wired into a runtime-host-owned WebRTC publisher and
the control plane can route session offers to it.

You can verify the same non-browser plumbing from SSH before opening the
browser:

```bash
cd ~/src/4-play
FOURPLAY_SEAT_API_TOKEN=<seat-api-token> \
python3 tools/check-webrtc-spike-status.py \
  --control-plane-url http://127.0.0.1:8080 \
  --publisher-health-url http://127.0.0.1:18080/health \
  --require-test-pattern-proxy
```

This checker does not create a WebRTC peer connection and does not prove media
decoding. It confirms that the control plane, protected readiness endpoint, and
optional local test-pattern publisher are reachable before the browser test.

Why `FOURPLAY_WEBRTC_BROWSER_HOST_IP` matters:

- Chrome/Edge often hide the Windows LAN address behind `.local` mDNS ICE
  candidates.
- In direct mode, the publisher can rewrite those candidates to the HTTP
  request source IP.
- In proxied mode, the publisher only sees the control plane at `127.0.0.1`.
- Therefore proxied spike tests need the browser/seat LAN IP configured
  explicitly, otherwise the publisher rewrites browser candidates to
  `127.0.0.1` and media remains stuck at a spinner.

If the proxy is not configured, the page will receive:

```text
webrtc_test_publisher_unavailable
```

You can still use the older direct path by setting `Publisher URL` back to:

```text
http://192.168.20.68:18080/offer
```

If Windows cannot reach `http://192.168.20.68:18080/health`, open the temporary
spike port from the Windows seat:

```bash
cd ~/src/4-play
deploy/systemd/open-webrtc-spike-port.sh --seat-ip 192.168.20.10 --apply
```

Or apply the equivalent rule directly:

```bash
sudo iptables -I INPUT -p tcp -s 192.168.20.10 --dport 18080 -j ACCEPT
```

Verify from Windows:

```powershell
Invoke-WebRequest http://192.168.20.68:18080/health
```

Expected response:

```text
4-Play WebRTC spike publisher ready
```

## Signal from the browser

Then click:

1. `Create local peer`
2. `Create SDP offer`
3. confirm `Publisher URL` is:

```text
http://192.168.20.68:18080/offer
```

4. `Signal publisher`

Leave `Include audio in this spike offer` unchecked for the first video-only
connectivity test. Add audio only after video packets are flowing.

The HTTP spike endpoint automatically rewrites browser `.local` mDNS ICE
candidates to the request's source IP. Chrome/Edge may put a `.local` hostname
in the offer instead of the Windows LAN IP. If the Linux publisher cannot
resolve that mDNS name, it may receive STUN packets but never answer them.

The publisher defaults to VP8 because Chrome/Edge typically offer VP8 first.
That keeps the spike simple and avoids dynamic payload-type mismatch while we
are still proving the transport. H.264 can be tested later with:

```bash
python3 tools/webrtc-gst-test-pattern.py \
  --codec h264 \
  --video-only \
  --http-signal 0.0.0.0:18080
```

## Add audio after video works

Stop the video-only publisher with `Ctrl-C`, then restart it without
`--video-only`:

```bash
cd ~/src/4-play
python3 tools/webrtc-gst-test-pattern.py \
  --http-signal 0.0.0.0:18080 \
  --answer-file /tmp/4play-webrtc-answer.sdp
```

In the browser:

1. click `Reset`
2. check `Include audio in this spike offer`
3. `Create local peer`
4. `Create SDP offer`
5. `Signal publisher`

Expected result:

- the moving video pattern still appears;
- the browser attaches two remote tracks, video and audio;
- an audio tick/beep is audible;
- receiver stats show increasing audio samples decoded.

The test publisher defaults Opus to RTP payload type `111`, matching the
payload type Chrome/Edge normally offer for browser WebRTC audio. If the answer
shows `a=inactive` in the audio section, confirm that the browser checkbox was
enabled before `Create local peer` and inspect the saved answer:

```bash
grep -n 'm=audio\|a=sendonly\|a=inactive\|rtpmap:111' /tmp/4play-webrtc-answer.sdp
```

For a successful audio negotiation, the audio section should include
`a=rtpmap:111 OPUS/48000/2` and `a=sendonly`.

## Plan the future raw MAME media pipeline

Before integrating real `session-runtime` media, inspect the planned
GStreamer pipeline separately:

```bash
cd ~/src/4-play
python3 tools/webrtc-gst-raw-media-plan.py --check-prereqs
python3 tools/webrtc-gst-raw-media-plan.py \
  --video-path /tmp/4play/session-example/video.raw \
  --audio-path /tmp/4play/session-example/audio.pcm \
  --width 320 \
  --height 224 \
  --fps-num 60 \
  --print-pipeline
```

For offline raw-shape validation without MAME, generate a short regular-file
sample:

```bash
mkdir -p /tmp/4play/raw-pattern
python3 tools/generate-raw-av-pattern.py \
  --video-path /tmp/4play/raw-pattern/video.raw \
  --audio-path /tmp/4play/raw-pattern/audio.pcm \
  --width 320 \
  --height 224 \
  --fps 60 \
  --duration-seconds 3
ls -lh /tmp/4play/raw-pattern/video.raw /tmp/4play/raw-pattern/audio.pcm
```

This tool does not launch a publisher and does not touch a live game. It keeps
the real-media WebRTC pipeline reviewable before we wire it into
runtime-host-owned session publishers.

## Real MAME raw-session publisher spike

The first true-MAME browser path is intentionally opt-in and side-by-side with
the existing UDP/FFplay path:

```text
MAME video.raw/audio.pcm FIFOs
  -> session-runtime MediaBridge
  -> existing FFmpeg UDP MPEG-TS path
  -> optional secondary WebRTC raw FIFOs
  -> tools/webrtc-gst-raw-session-publisher.py
  -> browser RTCPeerConnection
```

Do not point the WebRTC publisher at MAME's original `video.raw` and
`audio.pcm` FIFOs. FIFOs distribute bytes to one reader, not every reader. The
session runtime must tee the media into separate WebRTC FIFOs so the browser
experiment cannot steal frames from the proven FFmpeg path.

For the first manual validation, launch one session with secondary FIFO paths:

```bash
SESSION_ID=manual-webrtc-tmnt
SESSION_DIR="/tmp/4play/session-${SESSION_ID}"
mkdir -p "$SESSION_DIR"
mkfifo "$SESSION_DIR/webrtc-video.raw" "$SESSION_DIR/webrtc-audio.pcm"

target/release/session-runtime \
  --session-id "$SESSION_ID" \
  --rom tmnt \
  --width 320 \
  --height 224 \
  --fps 60 \
  --players 4 \
  --destination-ip 192.168.20.10 \
  --udp-port 41000 \
  --input-port 42000 \
  --input-token manual-webrtc-token \
  --mame-path "$HOME/src/mame-4play/mame" \
  --mame-ini-path /opt/4play/config/mame \
  --status-file "/tmp/4play/host-agent/${SESSION_ID}.status" \
  --webrtc-video-path "$SESSION_DIR/webrtc-video.raw" \
  --webrtc-audio-path "$SESSION_DIR/webrtc-audio.pcm" \
  --autosave
```

In another Linux terminal, start the raw-session publisher:

```bash
python3 tools/webrtc-gst-raw-session-publisher.py \
  --session-id "$SESSION_ID" \
  --video-path "$SESSION_DIR/webrtc-video.raw" \
  --audio-path "$SESSION_DIR/webrtc-audio.pcm" \
  --width 320 \
  --height 224 \
  --fps-num 60 \
  --http-signal 0.0.0.0:43000 \
  --browser-host-ip 192.168.20.10 \
  --answer-file /tmp/4play-webrtc-raw-session-answer.sdp
```

Then use the existing browser test page:

```text
http://192.168.20.68:8080/webrtc-test
```

Set `Publisher URL` to:

```text
http://192.168.20.68:43000/sessions/manual-webrtc-tmnt/offer
```

Expected result:

- the game remains playable through the existing UDP/FFplay path;
- the browser receives the same real MAME video and audio in its `<video>`
  surface;
- receiver stats increase without a growing buffer;
- if the WebRTC publisher fails, the FFplay path and MAME session continue.

This manual spike proves the raw media shape before host-agent-owned publisher
lifecycle is enabled.

## Manual fallback

If the automated endpoint fails, manual SDP copy/paste is still available:

```bash
cat > /tmp/4play-webrtc-offer.sdp <<'EOF'
PASTE_THE_BROWSER_OFFER_HERE
EOF

python3 tools/webrtc-gst-test-pattern.py \
  --video-only \
  --browser-host-ip 192.168.20.10 \
  --offer-file /tmp/4play-webrtc-offer.sdp \
  --answer-file /tmp/4play-webrtc-answer.sdp
```

Then paste `/tmp/4play-webrtc-answer.sdp` into the browser and click:

```text
Apply answer
```

If the browser reports an invalid SDP line, first confirm that the answer file
contains only SDP and no terminal prompts/markers:

```bash
grep -n 'fingerprint\|^v=\|^m=' /tmp/4play-webrtc-answer.sdp
python3 - <<'PY'
from pathlib import Path
answer = Path("/tmp/4play-webrtc-answer.sdp").read_text()
print(repr(answer.splitlines()[0]))
print("lines", len(answer.splitlines()))
PY
```

If the browser accepts the answer but no tracks/stats appear, check media
direction:

```bash
grep -n 'a=sendonly\|a=recvonly\|a=inactive\|rtpmap' /tmp/4play-webrtc-answer.sdp
```

For the first video-only test, the answer should contain one video `a=sendonly`
section and no audio section. If video contains `a=inactive`, the publisher did
not match an offered browser codec or track direction.

If `tcpdump` shows repeated inbound UDP packets from Windows to the Linux ICE
port but no outbound packets, inspect the offer candidate:

```bash
grep -n 'candidate\|ice-ufrag\|ice-pwd' /tmp/4play-webrtc-offer.sdp
```

If the candidate address ends in `.local`, rerun the publisher with the Windows
seat IP:

```bash
python3 tools/webrtc-gst-test-pattern.py \
  --video-only \
  --browser-host-ip 192.168.20.10 \
  --offer-file /tmp/4play-webrtc-offer.sdp \
  --answer-file /tmp/4play-webrtc-answer.sdp
```

Expected first-pass result:

- the video surface starts showing a GStreamer test pattern;
- for video-only baseline, this appears as a white ball/circle moving on a
  black background;
- connection state moves toward `connected`;
- receiver stats begin increasing;
- decoded frames increase;
- dropped frames may increase occasionally, but playback should not drift into a
  larger and larger delay.

## Validated result

On 2026-10-11, the automated VP8 video-only path succeeded on the reference
network:

- Windows seat `192.168.20.10`
- Linux runtime host `192.168.20.68`
- publisher URL `http://192.168.20.68:18080/offer`
- temporary TCP firewall opening for port `18080`
- visible browser result: white circle moving on black background

Detailed record:

- `docs/testing/WEBRTC-BROWSER-MEDIA-VALIDATION.md`

Latency/stability checklist:

- `docs/testing/WEBRTC-BROWSER-MEDIA-LATENCY-CHECK.md`

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
