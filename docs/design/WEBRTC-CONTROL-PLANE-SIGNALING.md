# WebRTC control-plane signaling plan

## Goal

Move from the standalone WebRTC spike publisher to a control-plane-owned
signaling flow that can eventually support real 4-Play sessions.

The successful spike proved browser-native video playback. This plan describes
the next architecture slice without changing the current UDP/FFplay gameplay
path.

## Current proven spike

```text
browser /webrtc-test
  -> POST SDP offer to standalone Python publisher /offer
  <- SDP answer
  -> browser <video>

Linux Python publisher
  -> GStreamer test pattern
  -> webrtcbin
```

Important behaviors:

- The browser can render live WebRTC video in-page.
- The browser may produce `.local` mDNS ICE candidates.
- The spike publisher rewrites `.local` candidates to the HTTP request source IP.
- The test currently opens a temporary TCP signaling port, usually `18080`.

## Next production-shaped slice

Add a control-plane signaling endpoint for experimental WebRTC sessions:

```text
browser
  -> POST /api/v1/webrtc-spike/sessions/{id}/offer
  <- answer

control plane
  -> validates auth
  -> forwards/coordinates offer with runtime host publisher
  -> returns answer

runtime host/session runtime
  -> owns GStreamer/WebRTC publisher lifecycle
```

For the next slice, the media can still be a test pattern. Do not attach MAME
raw media until signaling and lifecycle are predictable.

## API sketch

Experimental endpoint names should stay clearly temporary:

```http
POST /api/v1/experimental/webrtc/test-pattern/offer
Authorization: Bearer <seat token>
Content-Type: application/sdp

v=0
...
```

Response:

```http
200 OK
Content-Type: application/sdp

v=0
...
```

Failure responses:

- `400 invalid_sdp_offer`
- `409 publisher_busy`
- `502 publisher_failed`
- `504 publisher_timeout`

## Lifecycle requirements

- Publisher startup must be explicit and observable.
- Publisher shutdown must be reliable.
- Only one spike publisher should own a test port at a time unless explicitly
  configured otherwise.
- If a WebRTC publisher fails, the existing UDP/FFplay path must be unaffected.
- All experimental endpoints should be easy to remove or graduate.

## Security and deployment notes

- The standalone spike uses unauthenticated HTTP on a temporary port.
- Production control-plane signaling must require the existing seat token.
- Browser clipboard and some future browser APIs require a secure context; the
  packaged client shell or HTTPS should be evaluated before production.
- WebRTC media uses dynamic UDP ICE ports. Firewall policy must account for the
  final ICE strategy.

## Latency guardrails

Before becoming a default player path, WebRTC must show:

- no visible growing delay over 10 minutes;
- no input-flow regression in the native seat UI;
- reliable cleanup;
- stable CPU headroom;
- audio/video sync comparable to the current good FFplay behavior.

## Work breakdown

1. Keep standalone spike for fast iteration.
2. Add a control-plane experimental offer endpoint.
3. Move the Python/GStreamer publisher behind a runtime-host-owned process.
4. Return answers through the control plane.
5. Record latency and stability results.
6. Replace test pattern input with session-runtime video/audio only after the
   test-pattern path is stable.
7. Add client UI feature flag for browser-native media.
8. Preserve UDP/FFplay fallback until WebRTC passes real gameplay tests.
