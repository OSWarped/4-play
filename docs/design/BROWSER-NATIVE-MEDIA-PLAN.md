# Browser-native media implementation plan

## Progress on 2026-10-12

The reference user confirmed real MAME video playback and clean music after the
whole-frame `appsrc` bridge and sample-count audio timing changes. Browser gameplay
input is now implemented via an unordered WebRTC data channel, server-authorized
player slots, and the existing authenticated UDP receiver. Automated tests cover
the real SCTP-to-UDP transport; manual gameplay/latency confirmation is pending.
See [browser input validation](../testing/WEBRTC-BROWSER-INPUT.md) for deployment,
controls, and tests. The experimental publisher still supports one browser peer
per MAME session; concurrent browser players/spectators are subsequent work.

## Goal

Move the normal player/spectator video path from an external FFplay window into
the 4-Play client UI without increasing gameplay latency.

The target experience is:

```text
client UI
  ├─ game library / active sessions
  ├─ in-page game video
  ├─ join/spectate/actions overlay
  └─ keyboard/gamepad/touch input capture
```

FFplay and UDP MPEG-TS remain as the known-good fallback until browser-native
media proves itself under real gameplay.

## Non-negotiable latency requirements

Browser-native video must not ship as the default player path until it satisfies
these checks on the local network:

- controls still feel immediate while video is in-page/fullscreen;
- held direction and simultaneous button presses still work;
- audio remains synchronized to action within the range already achieved by the
  UDP/FFplay path;
- video does not introduce a persistent visible lag versus FFplay;
- frame drops are preferable to latency accumulation;
- session start/stop cleanup remains reliable;
- if the browser-native path fails, the client can fall back to UDP/FFplay.

Practical target for the first playable prototype:

- no perceptible input-to-photon regression versus FFplay;
- no growing buffer over time;
- no steady-state audio delay larger than the current good FFplay behavior.

## Architecture direction

WebRTC is the preferred player-seat media path.

```text
session-runtime / media publisher
  -> WebRTC media
  -> client RTCPeerConnection
  -> <video srcObject=remoteStream>

client input
  -> existing authenticated input channel initially
  -> optional WebRTC data channel later
```

The current UDP MPEG-TS path stays intact:

```text
session-runtime
  -> UDP MPEG-TS
  -> FFplay / OBS / diagnostics
```

This dual-path transition avoids risking the working game loop while WebRTC is
developed.

## Phase W0: Contracts and guardrails

Status: started.

Implemented:

- `MediaTransport` protocol enum:
  - `udp_mpeg_ts`
  - `web_rtc`
- runtime-host capability field:
  - `media_transports`
- session/spectator request field:
  - `media_transport`
- session/spectator grant field:
  - `media_transport`
- runtime assignment field:
  - `media_transport`
- serde defaults preserve legacy `udp_mpeg_ts` behavior;
- current runtime host advertises only `udp_mpeg_ts`;
- current clients explicitly request `udp_mpeg_ts`;
- control plane rejects `web_rtc` requests with `unsupported_media_transport`
  until the publisher exists.
- `/webrtc-test` static diagnostics page with:
  - browser WebRTC/WebCodecs/MSE capability checks;
  - recv-only `RTCPeerConnection` lifecycle scaffold;
  - manual SDP offer/answer fields for early publisher experiments;
  - browser receiver stats for decoded frames, dropped frames, jitter, RTT, and
    packet loss;
  - placeholder `<video>` surface for future remote streams;
  - explicit latency guardrail notes.

Exit criteria:

- existing FFplay gameplay unaffected;
- protocol tests confirm legacy JSON defaults to `udp_mpeg_ts`;
- control-plane tests confirm WebRTC fails fast instead of silently downgrading.

## Phase W1: Browser playback spike

Status: audio/video proof of concept passed on the reference LAN.

Goal: prove in-page video can meet the latency bar before touching the default
player flow.

Deliverables:

- `/webrtc-test` or equivalent development page;
- JavaScript client that can:
  - create an `RTCPeerConnection`;
  - create a recv-only SDP offer for manual or automated signaling;
  - exchange offer/answer through a test signaling endpoint;
  - attach the remote `MediaStream` to a `<video>`;
  - display connection state, jitter/packet stats, dropped frames when available;
- one host-side publisher path using the fastest available practical stack.

Implemented:

- `/webrtc-test` can create a recv-only peer for video-only or video+audio.
- Browser can signal the standalone publisher through HTTP `/offer`.
- Browser can signal the standalone publisher through the authenticated
  control-plane experimental WebRTC proxy.
- Runtime-host-agent can optionally supervise the test-pattern publisher
  lifecycle with `FOURPLAY_WEBRTC_TEST_PATTERN_SUPERVISED=1`, which is the
  first step away from a standalone side service.
- The control plane exposes the production-shaped
  `POST /api/v1/sessions/{session_id}/webrtc/offer` contract.
- Session WebRTC signaling still fails closed by default, but the control plane
  can now proxy offers to an explicitly configured
  `FOURPLAY_WEBRTC_SESSION_PUBLISHER_URL_TEMPLATE` once a runtime-host-owned
  publisher service exists.
- `ConnectionGrant` and `SpectatorGrant` include optional token-safe
  `webrtc_signaling` metadata for future browser-native media handoff.
- Browser attaches the remote stream directly to an in-page `<video>`.
- Browser diagnostics log track attach/mute/unmute/end events.
- Receiver stats include decoded frames, dropped frames, audio samples decoded,
  packet loss, jitter, and RTT where available.
- The browser-served seat client now shows WebRTC readiness from the control
  plane and includes an in-page receiver scaffold that can post an SDP offer to
  a token-safe per-session `webrtc_signaling.offer_path` when a future runtime
  host supplies one.
- `tools/webrtc-gst-test-pattern.py` publishes a VP8 video test pattern and
  optional Opus audio ticks through GStreamer `webrtcbin`.
- `tools/webrtc-gst-raw-media-plan.py` records the planned GStreamer pipeline
  for future MAME raw FIFO input (`video.raw` BGR0/BGRX frames and `audio.pcm`
  signed 16-bit stereo PCM) without changing the live session-runtime path.
- `session-runtime` can optionally tee its existing raw MAME media bridge to
  secondary WebRTC raw FIFOs with `--webrtc-video-path` and
  `--webrtc-audio-path`. This preserves the current UDP/FFplay path and avoids
  attaching multiple readers to MAME's original FIFOs.
- `tools/webrtc-gst-raw-session-publisher.py` is the first true-MAME publisher
  spike. It reads the secondary raw FIFOs, encodes VP8/Opus with GStreamer
  `webrtcbin`, and accepts SDP offers on `/sessions/{session_id}/offer`.
- The publisher rewrites browser `.local` mDNS candidates to the HTTP request
  source IP for this spike.
- When signaling is proxied through the control plane, the publisher accepts
  `FOURPLAY_WEBRTC_BROWSER_HOST_IP` so candidate rewriting uses the real browser
  seat IP instead of `127.0.0.1`.
- The publisher defaults Opus payload type to `111`, matching Chrome/Edge's
  usual WebRTC Opus offer.

Validated:

- On 2026-10-11, the browser rendered the generated VP8 video test pattern.
- On 2026-10-11, the browser also received audible generated Opus audio ticks
  with the same in-page media surface.
- On 2026-10-11, the browser received generated audio/video through the
  authenticated control-plane proxy after setting
  `FOURPLAY_WEBRTC_BROWSER_HOST_IP=192.168.20.10`.

Candidate publisher stacks, in preferred experiment order:

1. GStreamer WebRTC pipeline if available on the Linux host.
2. Rust WebRTC publisher if dependency and codec integration are practical.
3. FFmpeg to a small WHIP/WebRTC gateway.

Latency gates:

- compare WebRTC test pattern latency against FFplay test pattern;
- confirm no growing delay after 10 minutes;
- confirm audio/video sync with an av-sync test pattern;
- record CPU load and dropped frames.

Current host readiness note:

- `deploy/systemd/check-browser-media-prereqs.sh` validates the GStreamer and
  Python GI packages needed for the spike.
- Do not enable `media_transport: "web_rtc"` in normal clients until the host
  advertises WebRTC support and the latency gates pass.

## Phase W2: Runtime-host WebRTC capability

Goal: make WebRTC a first-class optional media transport.

Control-plane changes:

- hosts advertise `web_rtc` only when the publisher stack is available;
- session creation may request `media_transport: "web_rtc"`;
- control plane allocates or returns a WebRTC signaling handoff;
  - first contract is implemented and guarded:
    `POST /api/v1/sessions/{session_id}/webrtc/offer`;
- grants expose only safe browser-facing signaling metadata.
  - protocol field is in place as optional `webrtc_signaling`;

Runtime-host changes:

- launch WebRTC publisher per session or per spectator grant;
  - first manual tool exists as `tools/webrtc-gst-raw-session-publisher.py`;
  - `runtime-host-agent` can now start a raw-session publisher beside each
    launched session when `FOURPLAY_WEBRTC_SESSION_PUBLISHERS=1`;
  - the initial routing model is deterministic:
    `webrtc_signal_port = 43000 + (input_udp_port - 42000)` by default;
  - the control plane session publisher template supports
    `{webrtc_signal_port}`, `{session_id}`, `{input_udp_port}`,
    `{media_udp_port}`, and `{runtime_host_id}` placeholders;
- bind publisher lifecycle to session lifecycle;
- expose health/state to the agent;
- cleanly stop publishers on session stop, runtime loss, and stale cleanup.

Latency gates:

- WebRTC session must not slow or destabilize existing UDP session launch;
- UDP and WebRTC can run side-by-side for the same session during migration;
- a failed WebRTC publisher must not kill the MAME session if UDP fallback is
  still available.

## Phase W3: Client UI integration

Goal: replace FFplay for normal player/spectator use when WebRTC is available.

Client changes:

- request `web_rtc` only when the user/client is configured for browser media;
- create `RTCPeerConnection` from the safe handoff;
- render in-page `<video>`;
  - initial scaffold is implemented in `/client` and stays dormant until grants
    include `webrtc_signaling`;
- keep overlay/menu controls in the same UI surface;
- allow fullscreen on the page/video element;
- keep FFplay fallback button/path.

Input strategy:

- keep the existing authenticated input channel initially to avoid changing two
  latency-sensitive systems at once;
- later evaluate WebRTC data channels for input only if they improve deployment
  or reliability without adding latency.

Latency gates:

- gameplay with in-page video must feel as tight as current FFplay;
- keyboard focus must remain correct in windowed and fullscreen modes;
- gamepad input must keep working when video is focused/fullscreen;
- Esc/Stop must tear down video and session cleanly.

## Phase W4: Producer and mobile follow-through

After player seats are stable:

- Admin/Producer console can open WebRTC spectator feeds in-page;
- OBS can use a browser source for WebRTC spectator/capture pages;
- mobile clients can use the same browser video plus touch overlay model;
- UDP MPEG-TS remains available for OBS/diagnostics where it is still useful.

## Current next development steps

1. Run the W1 audio/video path for 5-10 minutes and record stats/CPU.
2. Validate runtime-host-agent-owned test-pattern publisher startup, restart,
   and shutdown on the Linux host.
3. Validate the raw-media WebRTC pipeline plan against throwaway FIFOs.
4. Feed real session-runtime MAME media into the WebRTC publisher.
5. Move from the manual raw-session publisher to runtime-host-owned
   per-session/per-spectator WebRTC publisher lifecycle.
6. Add client UI feature flag for browser media while keeping UDP/FFplay
   fallback.
