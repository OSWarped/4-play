# Client cover-flow refresh (2026-10-15)

V2 retains the working media/input/session lifecycle while refreshing the library
with shallower cover perspective, restrained mint accents, a dim screenshot
backdrop, larger screenshot-first covers, and contained logo/marquee images.
V1 retains its original layout and benefits from shared title/artwork handling.

## Selection regression

The old V2 scroll handler measured transformed `getBoundingClientRect()` centers,
while recentering used layout offsets (and incorrectly subtracted the rail's own
offset). A selection changed those transformed bounds, feeding selection changes
back into scrolling. Native CSS snap also used transformed cover bounds.

Both operations now use untransformed card centers in the positioned rail's
scroll coordinate system. V2 uses explicit settle selection instead of native
snap. Programmatic centering cannot change selection; pointer/touch/wheel intent
returns control to user scrolling. Adjacent navigation animates, while jumps
over two viewports are instant rather than sweeping through dozens of games.
Reduced-motion preferences are honored.

Unchanged catalog refreshes preserve cover nodes and artwork loaders rather than
rebuilding them every 15 seconds. Repeated centering of the same card/viewport
does not interrupt a swipe. Search and changed metadata still update the rail.

## Titles and artwork

- Nonblank trimmed `metadata.sort_title` is the main title, with display name
  and game ID fallbacks. This does not rename ROMs or change session game IDs.
- Library search includes both original and sort titles. Cards, featured view,
  game details and session headings use the preferred title.
- Covers prefer screenshots, then artwork, then marquee/logo fallback. Screenshots
  use `contain` so the full frame is visible rather than cropped into a poster.
- A successfully decoded featured logo replaces both the marquee and text title,
  with the preferred title retained in its accessible image description. A missing
  or failed logo restores the title and any successfully loaded marquee. The
  presence of a path/HTTP response alone never hides the text fallback.
- Asset loading retains authenticated same-origin requests, size/type/path
  checks, stale-request cancellation and object URL cleanup. Only nearby covers
  load images; no change to WebRTC or game input buffering.

## Automated validation

```powershell
node --test tools/test-*.cjs
node tools/check-client-coverflow-browser.cjs
```

Pass Chrome/Edge's executable as the optional final argument when necessary.
The real-browser check runs against local synthetic fixtures, never live sessions:
all 46 cover selections and physical centers, sort titles, logo/marquee visibility,
unchanged-refresh node identity, and portrait/landscape width constraints. It
captures `tmp/client-modern-{desktop,phone,landscape}.png` for visual inspection.
Chromium runs hidden/headless with its own temporary profile, retained for
diagnostics. No normal browser profile is used or deleted.

On the reference Windows PC, all 69 Node tests and the Chromium check passed.
This does not replace a Safari touch-device check.

## Deployment and manual check

Static assets are compiled into `control-plane-server`. After building/pulling:

```bash
sudo systemctl restart 4play-control-plane.service
```

Hard-refresh `/client-v2` (Ctrl+F5 on Windows). Check the previously unselectable
titles, arrows, search, touch swipes, and rapid changes. Verify sort titles match
Admin metadata and logo/marquee fallbacks behave with your actual artwork.
Start/join/watch should behave as before; verify an iPhone landscape game still
uses the dedicated player surface with touch controls.
