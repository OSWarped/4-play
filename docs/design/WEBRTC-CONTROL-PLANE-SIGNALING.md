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
- The browser can receive generated Opus audio in the same media element.
- The browser may produce `.local` mDNS ICE candidates.
- The spike publisher rewrites `.local` candidates to the HTTP request source IP.
- The test currently opens a temporary TCP signaling port, usually `18080`.
- Chrome/Edge normally offer Opus as dynamic payload type `111`; the publisher
  must honor the negotiated payload type instead of assuming its own static
  value.

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

## Implemented experimental proxy

The control plane now exposes an authenticated experimental test-pattern proxy:

```http
POST /api/v1/experimental/webrtc/test-pattern/offer
Authorization: Bearer <seat token>
Content-Type: application/json
```

Body:

```json
{
  "seat_id": "webrtc-test-seat",
  "role": "spectator",
  "sdp_offer": "v=0\r\n..."
}
```

Response:

```json
{
  "session_id": "experimental-test-pattern",
  "media_transport": "web_rtc",
  "sdp_answer": "v=0\r\n...",
  "expires_unix_ms": 123456789
}
```

This endpoint is intentionally disabled unless the control-plane process is
configured with:

```bash
FOURPLAY_WEBRTC_TEST_PATTERN_PUBLISHER_URL=http://127.0.0.1:18080/offer
FOURPLAY_WEBRTC_BROWSER_HOST_IP=192.168.20.10
```

When disabled, the endpoint returns `503 webrtc_test_publisher_unavailable`.
When enabled, it forwards only the SDP offer to the standalone
`tools/webrtc-gst-test-pattern.py` publisher and returns the answer inside the
typed JSON envelope.

The browser host IP is required for the current proxied spike because Chrome
and Edge commonly publish `.local` mDNS ICE candidates instead of the Windows
LAN IP. The standalone publisher can rewrite those mDNS candidates, but through
the control-plane proxy its request source is `127.0.0.1`; without an explicit
`FOURPLAY_WEBRTC_BROWSER_HOST_IP`, it rewrites the browser candidates to
localhost and media never flows.

## Implemented session endpoint contract

The control plane now exposes the production-shaped session endpoint, but it is
deliberately fail-closed until runtime-host-owned per-session publishers exist:

```http
POST /api/v1/sessions/{session_id}/webrtc/offer
Authorization: Bearer <seat token>
Content-Type: application/json

{
  "seat_id": "windows-seat-1",
  "role": "player",
  "player_number": 1,
  "sdp_offer": "v=0\r\n..."
}
```

Current behavior:

- validates the seat identity and SDP offer;
- verifies the session exists;
- rejects terminal sessions with `409 session_not_active`;
- returns `503 webrtc_session_publisher_unavailable` for valid nonterminal
  sessions until the runtime host can create a real WebRTC publisher.

The expected future success response remains:

Response:

```http
200 OK
Content-Type: application/json

{
  "session_id": "session-one",
  "media_transport": "web_rtc",
  "sdp_answer": "v=0\r\n...",
  "expires_unix_ms": 123456789
}
```

For spectators and producer feeds, use the same offer/answer shape but change
the role and supply the matching grant identity when needed:

```json
{
  "seat_id": "producer-console",
  "role": "producer",
  "spectator_grant_id": "grant-one",
  "sdp_offer": "v=0\r\n..."
}
```

The shared protocol types for this future API are:

- `WebRtcSeatRole`
- `WebRtcOfferRequest`
- `WebRtcAnswerResponse`
- `WebRtcSignalingGrant`

`ConnectionGrant` and `SpectatorGrant` now have an optional
`webrtc_signaling` field. It is omitted for the current UDP/FFplay path. When
WebRTC is enabled for a session or spectator grant, this field should contain
only browser-safe signaling metadata, such as:

```json
{
  "offer_path": "/api/v1/sessions/session-one/webrtc/offer",
  "expires_unix_ms": 123456789
}
```

It must never include the private input token.

Failure responses:

- `400 invalid_sdp_offer`
- `401 unauthorized`
- `404 session_not_found`
- `409 session_not_active`
- `503 webrtc_session_publisher_unavailable`
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
