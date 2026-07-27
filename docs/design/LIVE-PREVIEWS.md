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

The UI should prioritize the selected or focused session. It need not decode full-motion video for every visible card simultaneously.

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
