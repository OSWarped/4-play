# Browser-native media implementation plan

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

- On 2026-10-11, `192.168.20.68` did not have `gst-launch-1.0` or
  `gst-inspect-1.0` installed.
- Use `deploy/systemd/check-browser-media-prereqs.sh` on the Linux runtime host
  before attempting the GStreamer publisher spike.
- Do not enable `media_transport: "web_rtc"` in normal clients until the host
  advertises WebRTC support and the latency gates pass.

## Phase W2: Runtime-host WebRTC capability

Goal: make WebRTC a first-class optional media transport.

Control-plane changes:

- hosts advertise `web_rtc` only when the publisher stack is available;
- session creation may request `media_transport: "web_rtc"`;
- control plane allocates or returns a WebRTC signaling handoff;
- grants expose only safe browser-facing signaling metadata.

Runtime-host changes:

- launch WebRTC publisher per session or per spectator grant;
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

1. Finish W0 contracts and commit.
2. Decide the first publisher stack for the Linux host.
3. Build the W1 test publisher.
4. Wire `/webrtc-test` to the test signaling/publisher path.
5. Measure against the existing FFplay path before enabling any default client
   behavior.
