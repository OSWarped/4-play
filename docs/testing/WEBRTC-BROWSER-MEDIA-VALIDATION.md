# WebRTC browser media validation

## 2026-10-11 video-only spike result

Status: passed for browser-native video proof of concept.

Environment:

- runtime host: `blake-web`, `192.168.20.68`
- Windows seat/client: `192.168.20.10`
- browser page: `http://192.168.20.68:8080/webrtc-test`
- publisher tool: `tools/webrtc-gst-test-pattern.py`
- publisher signaling URL: `http://192.168.20.68:18080/offer`
- publisher mode: `--video-only`
- video codec: VP8

Observed success:

- Browser created a local recv-only `RTCPeerConnection`.
- Browser generated a video-only SDP offer.
- Browser successfully reached the Linux publisher over HTTP `/offer`.
- Linux publisher generated and returned an SDP answer.
- Browser applied the answer automatically.
- Browser rendered live WebRTC video in the page.
- Visible result: white circle moving on a black background in the page video
  surface.

Important setup discovered:

- Linux GStreamer/WebRTC packages and Python GI bindings were required.
- Browser copy-to-clipboard failed over plain HTTP because the page is not a
  secure context; this is acceptable for the spike.
- Manual copy/paste SDP was too brittle for ICE/mDNS behavior.
- Chrome/Edge produced browser host candidates as `.local` mDNS names.
- The automated HTTP `/offer` endpoint worked because it could rewrite mDNS
  candidates to the request source IP.
- TCP port `18080` on Linux needed to be reachable from the Windows seat.

Temporary firewall rule used:

```bash
sudo iptables -I INPUT -p tcp -s 192.168.20.10 --dport 18080 -j ACCEPT
```

Validation command sequence:

```bash
cd ~/src/4-play
git pull origin main
cargo build --release -p control-plane-server
sudo systemctl restart 4play-control-plane.service
python3 tools/webrtc-gst-test-pattern.py \
  --video-only \
  --http-signal 0.0.0.0:18080
```

Windows reachability check:

```powershell
Invoke-WebRequest http://192.168.20.68:18080/health
```

Expected response:

```text
4-Play WebRTC spike publisher ready
```

Conclusion:

Browser-native video is viable for 4-Play. The next work should not focus on
whether `<video>` can display this stream; it should focus on production-grade
signaling, lifecycle ownership, audio, latency stability, and integration with
real session-runtime media.

## 2026-10-11 audio + video spike result

Status: passed for browser-native generated audio/video proof of concept.

Validation command sequence:

```bash
cd ~/src/4-play
git pull origin main
cargo build --release -p control-plane-server
sudo systemctl restart 4play-control-plane.service
python3 tools/webrtc-gst-test-pattern.py \
  --http-signal 0.0.0.0:18080 \
  --answer-file /tmp/4play-webrtc-answer.sdp
```

Browser sequence:

1. hard-refresh `http://192.168.20.68:8080/webrtc-test`
2. `Reset`
3. enable `Include audio in this spike offer`
4. `Create local peer`
5. `Create SDP offer`
6. `Signal publisher`

Observed success:

- Browser rendered the live VP8 test pattern in-page.
- Browser attached both remote video and remote audio tracks.
- Opus audio ticks/beeps were audible.
- The publisher generated an answer with audio negotiated as send-only after
  aligning the Opus payload type with browser payload type `111`.

Conclusion:

Browser-native WebRTC media is viable for generated video and audio on the
reference network. The next work should move from standalone spike signaling
toward control-plane-owned signaling and runtime-host-owned publisher
lifecycle, while retaining UDP/FFplay as the gameplay fallback.

## Remaining spike checks

## 2026-10-11 control-plane-proxied audio + video result

Status: passed for browser-native generated audio/video through the
control-plane experimental WebRTC proxy.

Validated path:

```text
browser /webrtc-test
  -> POST /api/v1/experimental/webrtc/test-pattern/offer
  -> control plane
  -> local publisher http://127.0.0.1:18080/offer
  <- SDP answer
  -> browser <video>
```

Environment additions:

```bash
FOURPLAY_WEBRTC_TEST_PATTERN_PUBLISHER_URL=http://127.0.0.1:18080/offer
FOURPLAY_WEBRTC_BROWSER_HOST_IP=192.168.20.10
```

Observed success:

- Browser sent the SDP offer to the authenticated control-plane endpoint.
- Control plane returned a typed `web_rtc` answer for
  `experimental-test-pattern`.
- Browser attached remote video and audio tracks.
- Browser rendered the test pattern and played generated audio.

Important setup discovered:

- Through the proxy, the standalone publisher sees the request source as
  `127.0.0.1`.
- Chrome/Edge still put the real browser ICE address behind `.local` mDNS
  candidates.
- Without `FOURPLAY_WEBRTC_BROWSER_HOST_IP`, the publisher rewrote browser ICE
  candidates to `127.0.0.1`, causing a spinner with attached tracks but no media.
- Setting `FOURPLAY_WEBRTC_BROWSER_HOST_IP=192.168.20.10` made the publisher
  rewrite candidates to the real Windows seat IP, and media flowed.

Conclusion:

The production-shaped signaling path is viable for generated WebRTC media.
The next work is runtime-host-owned publisher lifecycle and then feeding real
session-runtime media into the WebRTC publisher.

## Remaining spike checks

- 5-10 minute latency/buffer stability observation.
- Browser stats recording during the test.
- CPU observation on the Linux host.
- H.264 experiment after VP8 baseline is stable.
