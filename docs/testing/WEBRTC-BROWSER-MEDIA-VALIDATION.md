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

## Remaining spike checks

- Audio test with `Include audio in this spike offer` enabled. The publisher
  now defaults Opus to browser payload type `111`; save the generated HTTP
  answer with `--answer-file /tmp/4play-webrtc-answer.sdp` and confirm the
  audio section answers `a=sendonly`.
- 5-10 minute latency/buffer stability observation.
- Browser stats recording during the test.
- CPU observation on the Linux host.
- H.264 experiment after VP8 baseline is stable.
