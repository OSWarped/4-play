# ADR-0006: Browser-native media path investigation

## Status

Proposed

## Context

The packaged native seat client currently launches FFplay as an external media
receiver. This is working, but it has two product-shape drawbacks:

- the player experience is split between the seat UI and a separate FFplay
  window;
- fullscreen or borderless FFplay can steal focus from the browser-hosted seat
  UI, which currently owns keyboard/gamepad input capture.

The desired end state is a single player-facing UI surface where the game video,
menus, overlays, active-session controls, and input capture live together. That
would make the product feel much more like Batocera/RetroPie-style frontends and
would simplify spectator/join overlays.

Current runtime media shape:

```text
MAME raw video/audio
  -> session-runtime FFmpeg
  -> H.264 video + AAC audio
  -> MPEG-TS
  -> UDP to seat/client machine
  -> FFplay or OBS
```

A browser cannot directly attach the current `udp://...` MPEG-TS URL to a
`<video>` element. Browser-native playback requires changing the transport,
container, or decode path.

## Browser media options considered

### Option A: WebRTC media stream

Shape:

```text
MAME raw video/audio
  -> runtime encoder
  -> WebRTC publisher
  -> browser RTCPeerConnection
  -> <video srcObject=MediaStream>
```

Pros:

- Designed for low-latency realtime audio/video.
- Browser playback uses a normal `<video>` element fed by a `MediaStream`.
- Fullscreen can happen inside the same page/app surface, preserving a path for
  overlays and focus management.
- Handles jitter buffering, A/V synchronization, congestion control, and NAT
  traversal better than a custom stack.
- Works for player seats and producer/spectator views.

Cons:

- Requires signaling in the control plane.
- Requires a WebRTC publisher in the runtime host/session runtime.
- Requires deciding whether the runtime uses a Rust WebRTC stack, GStreamer,
  FFmpeg WebRTC support where available, or an external media gateway.
- More moving parts than the current UDP/FFplay path.

Assessment: best fit for the eventual product.

### Option B: Media Source Extensions over HTTP/WebSocket

Shape:

```text
MAME raw video/audio
  -> runtime encoder
  -> fragmented MP4 or WebM stream
  -> browser MediaSource/SourceBuffer
  -> <video>
```

Pros:

- Uses a real `<video>` element.
- Can keep an HTTP/WebSocket control-plane-friendly architecture.
- H.264/AAC in fragmented MP4 is a common MSE baseline.

Cons:

- Low latency is harder than WebRTC. Segmenting, appending, buffering, and
  trimming all become our responsibility.
- The current MPEG-TS over UDP stream still needs to be re-muxed or replaced.
- Browser/container compatibility needs active testing.
- Risk of creeping back toward streaming-video latency instead of arcade
  latency.

Assessment: viable for spectator previews or less latency-sensitive modes, but
not the preferred player-seat path.

### Option C: WebCodecs with custom transport

Shape:

```text
MAME raw video/audio
  -> encoded chunks
  -> WebSocket/WebTransport/custom framing
  -> browser WebCodecs VideoDecoder/AudioDecoder
  -> Canvas/WebGL/WebAudio output
```

Pros:

- Potentially very low latency.
- Full control over buffering, frame dropping, timing, rendering, and overlays.
- Could eventually support advanced rendering paths.

Cons:

- We would own muxing/framing, decode queues, A/V sync, audio scheduling,
  backpressure, error recovery, and browser compatibility.
- More engineering risk than WebRTC.
- WebTransport has attractive datagram semantics but adds HTTPS/HTTP3 and
  deployment complexity.

Assessment: interesting long-term research path, not the shortest route to a
solid product.

### Option D: HLS / low-latency HLS / DASH

Pros:

- Mature distribution model.
- Good for broadcast/spectator use cases.

Cons:

- Latency profile is generally a poor fit for player-controlled arcade gameplay.
- Operational complexity does not buy us much for local-network seats.

Assessment: not appropriate for player seats; may be useful for YouTube-style
producer/broadcast pipelines later.

## Decision direction

Keep FFplay as the Version 1 fallback while beginning a WebRTC media prototype.

WebRTC should become the target browser-native media path because it aligns best
with:

- low-latency player seats;
- one-window UI/overlay/fullscreen experience;
- future mobile/tablet clients;
- producer/spectator views;
- no external FFplay window;
- preserving browser-native keyboard/gamepad/touch UI flows.

The current UDP MPEG-TS output should remain available during the transition
because it already works with FFplay and OBS and is valuable for diagnostics and
production feeds.

## Proposed implementation slices

### Slice 1: WebRTC spike outside the main runtime

Goal: prove that a browser can render a low-latency live stream from the Linux
host in a `<video>` element.

Possible spike inputs:

- FFmpeg test pattern (`testsrc2` + `sine`);
- MAME-derived raw video/audio from one session;
- existing session-runtime output re-encoded/re-published through a temporary
  gateway if needed.

Deliverables:

- `/webrtc-test` page or small standalone static page;
- signaling endpoint in the control plane or a small test signaling server;
- one Linux publisher process;
- browser `<video>` playback with measured glass-to-glass feel;
- notes on CPU load, latency, audio/video sync, and browser compatibility.

### Slice 2: Runtime-host WebRTC capability

Goal: add an optional browser-media capability without removing the current UDP
media path.

Control-plane additions:

- runtime host advertises `media_transports: ["udp_mpegts", "webrtc"]`;
- session grants can include a sanitized WebRTC offer URL or signaling route;
- spectator grants can request either UDP/OBS style output or WebRTC browser
  output.

Runtime additions:

- session-runtime can start a WebRTC publisher for a session;
- lifecycle follows session start/stop;
- per-session/per-spectator resources are cleaned up safely.

### Slice 3: Native seat UI embeds browser video

Goal: replace FFplay for normal player use while keeping FFplay as a fallback.

Client additions:

- native UI starts/join/spectates and receives a WebRTC handoff;
- JavaScript creates `RTCPeerConnection`;
- remote stream is attached to an in-page `<video>`;
- keyboard/gamepad/touch input remains in the same UI surface;
- overlay menu can animate over the video;
- fullscreen can be implemented on the page/video element rather than a
  separate process.

### Slice 4: Retire FFplay from default player flow

Goal: make WebRTC the default for player seats once it is stable.

Keep:

- UDP MPEG-TS for OBS, producer feeds, diagnostics, and fallback;
- FFplay fallback for environments where WebRTC is unavailable.

## Open questions

- Which WebRTC publisher stack should we use?
  - Rust-native WebRTC library;
  - GStreamer pipeline;
  - FFmpeg plus a WebRTC/WHIP gateway;
  - dedicated local media server/gateway.
- Should input stay over the existing authenticated UDP route, move to WebSocket,
  or use WebRTC data channels?
- Do we need one publisher per viewer, or can a session publisher support
  multiple subscribers efficiently?
- How do we expose WebRTC spectator feeds for OBS? OBS can ingest browser
  sources, but dedicated production feeds may still prefer UDP/SRT/RTMP.
- What is the acceptable latency budget for browser-native player video?
- Which browsers are first-class targets for the packaged client shell?

## References

- MDN WebRTC API: https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API
- MDN WebRTC signaling/video calling example, including assigning remote streams
  to a `<video>` element with `srcObject`:
  https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Signaling_and_video_calling
- MDN Media Source API: https://developer.mozilla.org/en-US/docs/Web/API/Media_Source_Extensions_API
- MDN SourceBuffer examples and MIME codec pattern:
  https://developer.mozilla.org/en-US/docs/Web/API/SourceBuffer
- MDN WebCodecs API: https://developer.mozilla.org/en-US/docs/Web/API/WebCodecs_API
- W3C WebTransport repository:
  https://github.com/w3c/webtransport
