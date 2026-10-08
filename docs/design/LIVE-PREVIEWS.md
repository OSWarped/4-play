# Live Preview Design

## Why previews matter

Live previews are not decorative. They let a player see what is happening elsewhere in the arcade, decide whether a session is interesting, and determine whether joining or spectating is worthwhile.

The preview system is therefore part of the Product MVP, even though it is not required for the Phase 1A remote-play feasibility experiment.

## Product behavior

An active-session card may show:

- live motion preview
- periodically refreshed still frame
- package artwork when live preview is unavailable
- session health and preview status

The control plane exposes a seat-safe active-session summary feed for browsing
clients. The feed includes player slots, spectator counts, and preview status
without exposing the per-session player connection grant or input token. The
current development preview status reports whether full-quality spectator media
can be requested; it is a bridge toward lower-cost preview streams, not the
final preview transport.

When live/spectator preview media is not available yet, the summary feed can
return an artwork fallback path from catalog metadata. The current fallback
priority is screenshot, artwork, marquee, then logo. This lets a browser render
a meaningful active-session card before the low-cost live preview transport is
complete, and gives future clients a graceful degradation path.

The UI should prioritize the selected or focused session. It need not decode full-motion video for every visible card simultaneously.

## Current development behavior

The Phase 2 development client can join an active player slot and route input
to that slot. Joined seats run input-only by default so multiple PowerShell
clients on one Windows development PC do not collide while binding the same
local gameplay UDP media port. A separate physical seat or experiment may pass
`--joined-media` to launch its own FFplay receiver. The development client also
offers `s<session-number>` spectator selection, which opens media without
reserving a player slot or sending input. Spectator selection requests a
spectator grant and binds the grant's distinct media UDP port. Both joined
media and spectator modes still use the full-quality gameplay stream; they are
not the final preview/spectator design.

The control plane now has a spectator-grant API that allocates a distinct
viewer media UDP port for an active session. The grant model is intentionally
separate from player-slot ownership. Runtime-host assignment responses include
the active spectator media ports for each session. Runtime media duplication to
those viewer ports is handled by a Rust fan-out loop fed by FFmpeg's MPEG-TS
stdout. The runtime host refreshes a per-session spectator port file during
reconciliation, allowing already-running sessions to start sending to newly
granted spectator ports without restarting MAME or FFmpeg.

This keeps player-slot and input work testable while preserving the Product MVP
requirement that preview or spectator media must not block joining or degrade
gameplay.

## Design goals

- one session preview can be consumed by many browsing seats
- preview generation does not require one full-quality encoder per viewer
- preview load never blocks or materially degrades gameplay
- preview failure falls back gracefully to still imagery or package artwork
- preview transport and authorization do not expose unrestricted runtime access
- the operator can disable previews globally or per session if needed

## Candidate approaches

Evaluate these in order of preference:

1. reuse the gameplay encode with a lower-rate or lower-resolution subscriber path
2. use one low-bitrate simulcast or secondary layer per active session
3. publish periodic JPEG or WebP frames from the runtime host
4. use package artwork when encoder or host capacity is constrained

The experiment should compare GPU encoder usage, CPU usage, bandwidth, decode cost on seats, startup delay, and the number of concurrent browsing clients supported.

## Open questions

- Should previews include audio? The initial assumption is no.
- How many moving previews should one browsing seat display at once?
- Should non-focused cards use still frames while the focused card uses motion?
- Can the selected gameplay transport expose an efficient reusable preview layer?
- How stale may a preview become before the UI labels it unavailable?
- What privacy or venue-policy controls are necessary?

## Product MVP acceptance criteria

The preview design is ready for Product MVP when:

1. multiple browsing seats can view the same active session without creating a dedicated full-quality encoder for each viewer
2. gameplay latency and frame delivery remain within their established targets
3. preview unavailability does not prevent starting, joining, spectating, or continuing a game
4. the browsing UI clearly distinguishes live, stale, fallback, and unavailable previews
5. operator controls can disable preview publication without terminating gameplay
