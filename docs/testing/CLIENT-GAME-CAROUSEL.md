# Client game carousel

The `/client` library now features one selected game above a horizontally
scrollable cover rail. Selection changes the artwork, description, year,
manufacturer, player count, and running-session indicator. Start game uses the
existing session-launch flow; Details & sessions uses the existing join/spectate
details. Browsing never starts a session or sends controller input.

## Interaction

- Previous/next wrap through the filtered library.
- Click a cover to feature its game. Scroll/swipe the rail to browse more covers.
- Tab to the selected cover; Left/Right move selection and focus, Home/End select
  the first/last result. Enter/Space select via the native button behavior.
- Search covers titles, ROM IDs, genre, manufacturer, and release year.
- Refresh preserves selection if the game remains in the filtered results.
- Empty or single-game results disable unnecessary navigation. Empty results
  disable launch/details. There is no autoplay or carousel background polling.
- Keyboard handlers are scoped to cover buttons, never the game video or forms.
- Cover transitions and rail scrolling respect reduced-motion preferences.

The featured image tries screenshot, artwork, marquee, then logo metadata. Assets
are fetched from the same-origin protected asset endpoint with the bearer header;
tokens are never put in image URLs. Aborted/stale responses cannot update a newly
selected game. Superseded object URLs are revoked. Missing or invalid artwork
leaves a CSS-rendered 4-Play fallback; a failed image decode also hides the image.
The rail uses styled title covers rather than eagerly downloading every asset.
No artwork scraping, AI raster assets, or external UI dependency is introduced.

## Automated checks

```powershell
node --test tools/test-client-carousel.cjs tools/test-browser-input.cjs tools/test-browser-media.cjs tools/test-browser-media-lifecycle.cjs tools/test-browser-spectator.cjs
cargo test -p control-plane-server -- --test-threads=1
```

Seven carousel tests cover selection, filtering, the shipped featured-view renderer,
safe asset paths, authenticated artwork requests, cleanup, stale response races
and fallback. All 29 browser tests
and 55 control-plane tests passed. The new module is served without API auth just
like the other UI scripts; catalog/media API authorization is unchanged.
Headless Edge rendering was inspected at desktop and narrow widths with a local
generated catalog. Page width checks confirmed no horizontal document overflow;
the rail itself deliberately scrolls. Live gameplay was not launched for preview.

## Activation and manual acceptance

The static client is embedded in the control-plane binary. After pulling and
building `cargo build --release -p control-plane-server` on Linux:

```bash
sudo systemctl restart 4play-control-plane.service
```

Prefer restarting between test games. No runtime-host-agent restart or firewall
change is needed. Refresh `/client` with Ctrl+F5. Check arrow/click/keyboard browsing,
search with no results, fallback and actual uploaded artwork, launch/details,
existing joining/spectating, and the new layout on your preferred display.
Physical joystick library navigation is a future increment; existing gameplay
and spectator-overlay gamepad handling is unchanged.
