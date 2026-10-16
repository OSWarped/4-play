# Client V4: catalog-first minimal draft

`/client-v4` is a separate experiment. V2 and V3 remain available. Authentication,
private seat grants, WebRTC and keyboard/gamepad/touch input reuse the existing
V2 facade and gameplay lifecycle. This page does not add service credentials.

## Visible interface

- No page title, introductory subtitle, permanent spotlight, genre labels on
  tiles, or floating play dock. The grid begins immediately below a compact
  **Search / Live / Filters** toolbar.
- Genre and clear controls live inside the Filters disclosure. Search and Live
  remain one-touch controls. Empty results provide a direct Clear filters action.
- Screenshots stay bright, full-frame, with titles below. Player counts and a
  conditional LIVE badge are the only tile overlays. Sort titles remain preferred.
- Clicking a game opens a native modal: right-side drawer on desktop, bottom
  sheet on phones. It contains the available photo, logo-only branding or title
  fallback, real metadata, controls, and immediate start/join/watch actions.
- Missing photos/descriptions do not create large generic placeholder blocks.
  Logo/marquee files are not reused as a duplicate photo in the drawer.
- Escape, the close button and backdrop dismissal return to browsing without
  launching. Native dialog semantics trap focus and make the background inert.
  The selected title remains the accessible dialog name even when a logo replaces
  visible title text. There is no hover-only interaction.
- Starting media closes the modal before gameplay takes focus. Leaving a game
  restores the browsing scroll position and tile focus. Connection errors remain
  visible as an actionable status banner despite the removed headings.

## Validation

```powershell
node --test tools/test-*.cjs
node tools/check-client-coverflow-browser.cjs --v4
cargo test -p control-plane-server
```

The browser fixture covers the header-free first viewport, filter composition,
desktop drawer/mobile sheet, Escape, session Join/Watch buttons, and modal closure
for the gameplay handoff. It creates no live sessions. Layout screenshots go to
`tmp/client-v4-*.png`; these use synthetic images, not scraped library artwork.

Server asset tests check V4 is separate and scripts initialize in V2/V3/V4/shared
order without embedding the API secret. Manual checks remain necessary for
Safari's actual touch keyboard, scrolling, dialog focus, rotation and gameplay.

After deploying the binary:

```bash
sudo systemctl restart 4play-control-plane.service
```

Open `/client-v4`. Test closing a game panel after scrolling far into the library,
starting/joining/watching, leaving gameplay, and recovering from an unavailable
game. V3 behavior should remain unchanged.
