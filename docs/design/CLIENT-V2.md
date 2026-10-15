# Client V2: walk up, choose, play

V2 is an independent draft at `/client-v2`. `/client` remains available unchanged
in setup/workflow. V2 reuses its tested WebRTC, input, touch, recovery, and
spectator-to-player lifecycle rather than creating a second media stack.

## End-user experience

1. Open the arcade's V2 URL. A seat identity and destination IP are automatic;
   there are no API-key, IP, or seat-name fields to complete.
2. Browse a perspective Cover Flow–style rail. The chosen cover moves forward,
   neighbors angle away, and title/details follow selection. Use arrows,
   keyboard, search, or swipe. Reduced-motion preference removes transitions.
   Artwork is loaded only around the selection; missing art retains title cards.
3. Start Game allocates once, waits for the runtime to become active, and opens
   the browser viewer and controls automatically. Startup failures and a 45 s
   wait deadline are explicit, not an invitation to start duplicate sessions.
4. Active sessions offer joining/rejoining a player or spectating. These actions
   open media automatically too. Spectators can join an open slot using the
   existing overlay; touch controls appear only after player authorization.
5. Leaving releases input and the owned slot or spectator grant, then returns to
   browsing. Existing session lifecycle policy still decides when MAME stops.
   Browsing refreshes every 15 s while visible and not playing/launching.

Some browsers may still require a Play / resume gesture for audio autoplay;
automatic launch cannot override a browser's media permission policy.

## Landscape correction

The old phone rule fixed video height at 40% of a short landscape viewport.
It now uses the available viewport height minus toolbar allowance. When touch
controls are present, the pad and action bank flank the centered video in the
same grid row rather than adding a large row beneath it. Portrait retains the
two-hand controller below the video. The correction is shared with V1.
No media encoder, buffering, RTP, or input heartbeat settings are changed.

## Zero-entry access, not a secret in JavaScript

Automatic enrollment is **off by default**. The operator explicitly configures
approved source networks AND exact site origins. Anyone on an approved network
can play through this draft. This is an operator trust decision, not individual
user authentication and not suitable as-is for untrusted public/guest Wi-Fi.

`POST /client-v2/bootstrap` issues a random browser seat identity in an HttpOnly,
SameSite=Strict cookie scoped to `/client-v2`. It has a seven-day expiry renewed
on bootstrap. The server signs it with HMAC-SHA256 using domain separation and
the configured seat secret. The identity survives reloads/server restarts while
the cookie is valid and the secret unchanged. Clearing cookies, using another
browser profile, expiry, or secret rotation means a new seat. Existing slot/grant
leases still expire through normal lifecycle rules.

The server-only `/client-v2/api/*` facade:

- Checks the real socket address against the operator's CIDRs. Forwarded-IP
  headers cannot enroll a seat or redirect its stream.
- Requires an approved Host and an explicit V2 request header. All writes must
  carry the exact configured same-origin Origin. No cross-origin CORS access.
- Replaces seat/destination fields with signed identity and actual socket IP;
  a browser cannot pick another seat or cause arbitrary UDP destinations.
- Allows only library/assets, active summaries, browser readiness, session
  creation, own-session reads/stops, slot/grant operations, signaling and leases.
  Existing slot/grant authorization remains authoritative for join and input.
- Does not expose metadata edits, host registration, global session/grant lists,
  production notes, scraper keys, or administrative cleanup.
- Keeps the shared bearer server-side and strips runtime `token` fields from
  JSON handoffs. Responses are no-store. Cookies do not authenticate the old
  `/api/v1` APIs or Admin UI.

The current server listens on HTTP. HttpOnly is not transport encryption: this
draft is for trusted LAN testing. Production needs HTTPS, a reviewed reverse
proxy identity policy, enrollment/rate/resource controls and revocation. Do NOT
add loopback/proxy networks or trust forwarded headers to work around this.
HTTPS origins get Secure cookies, but this change does not install TLS or
implement trusted-proxy support.

See [cookie configuration](https://developer.mozilla.org/en-US/docs/Web/Security/Practical_implementation_guides/Cookies)
and [CSRF guidance](https://developer.mozilla.org/en-US/docs/Web/Security/Attacks/CSRF).

## Activation and validation

See [CLIENT-V2-VALIDATION.md](../testing/CLIENT-V2-VALIDATION.md).
Future work: device enrollment/labels and revocation, public-venue authorization,
TLS deployment, per-game button labels/custom layouts, and a polished fullscreen
mobile HUD. These are not prerequisites for trying this separate LAN draft.
