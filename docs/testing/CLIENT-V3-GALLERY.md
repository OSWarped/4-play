# Client V3: screenshot gallery draft

Open `/client-v3` after deploying/restarting the control-plane server. `/client-v2`
retains its existing cover-flow layout and `/client` remains available.

V3 uses the same HttpOnly seat cookie, bootstrap, scoped API facade, launch flow,
WebRTC player and keyboard/gamepad/touch input as V2. It intentionally calls
`/client-v2/bootstrap` and `/client-v2/api/...`; there is no new authentication
mechanism, shared credential in the page, or duplication of media/input logic.
Opening V2 and V3 in the same browser uses the same seat identity, not a new seat.

## Experience

- Responsive screenshot-first tile grid rather than a horizontal carousel.
- Search composes with genre and active-game filters; clear resets all filters.
- Selected game spotlight uses sort title and logo-only branding when available.
  On phones the spotlight is compact so more of the catalog is visible.
- A fixed play/details bar keeps actions reachable while browsing deep in the
  library; these are the original buttons, not duplicated launch handlers.
- Details retain the existing start/join/watch actions. Up/down arrows move one
  grid row, left/right move between games; selection alone does not launch.
- Tile images load near the viewport. Changed/filtered-out cards release their
  requests and blob URLs. Existing games with no artwork retain text fallbacks.
- Gallery/actions disappear when browser gameplay starts. No new video buffers,
  transport changes or gameplay-input polling are introduced.

## Validation

```powershell
node --test tools/test-*.cjs
node tools/check-client-coverflow-browser.cjs --v3
cargo test -p control-plane-server
```

The Chromium fixture check exercises the 46-game grid, genre/live/clear filters,
empty-state disabling, selected titles, fixed action placement, desktop/phone/
landscape page width, and gallery removal during gameplay. Screenshots are saved
to `tmp/client-v3-{desktop,phone,landscape}.png`. It creates no live sessions.
Server tests verify separate V2/V3 pages/assets, script ordering, and absence of
embedded service credentials; the same seat boundary tests apply to both.

After a manual service restart, test with the real catalog and an iPhone:

```bash
sudo systemctl restart 4play-control-plane.service
```

Check scrolling, search, genres, active-game filtering, keyboard row movement,
start/join/watch and returning from a game. A layout experiment is not a claim
that physical touch devices or live gameplay have been retested automatically.
