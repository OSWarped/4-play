# Client V2 validation

## Operator activation (once per installation)

The control plane must already have working session WebRTC publishing. Pull and
build `control-plane-server` on Linux. In `/etc/4play/4play.env`, add/update:

```ini
FOURPLAY_BROWSER_SEAT_NETWORKS=192.168.20.0/24,192.168.0.0/24,192.168.10.0/24
FOURPLAY_BROWSER_SEAT_ORIGINS=http://192.168.20.68:8080
```

This explicitly permits automatic gameplay access from those trusted networks.
Do not use a public/guest subnet. List exact origins (comma-separated), not `*`.
Never put the API key in the HTML, a query string, or V2 browser storage.

```bash
sudo systemctl restart 4play-control-plane.service
```

No host-agent restart, new UDP range, or additional firewall rule is required
on networks already tested with browser media. Existing V1 setup still works.
To disable automatic enrollment/access, remove either setting and restart.

## Manual test

1. Open `http://192.168.20.68:8080/client-v2` in a new/private browser profile.
   No token, IP, or seat entry should be requested. Choose a game using the
   animated covers; filter and keyboard arrows should work. Missing artwork
   must not prevent browsing. Reduced motion should disable the transitions.
2. Click Start Game once. Verify progress followed by the browser viewer without
   copying commands or pressing another launch button. Check input and A/V sync.
   A browser autoplay restriction may still require Play / resume.
3. Open V2 in another device/browser profile. Join the existing game; verify
   independent player controls. Two tabs in one profile intentionally share a
   seat identity; use separate profiles/devices for independent-seat tests.
4. Spectate, then join an open slot through the spectator overlay. Verify touch
   controls only appear after joining. Leave and rejoin/reconnect.
5. On the iPhone, rotate to landscape. The video should use a taller center
   area with controls to its sides, rather than the former 40%-height window.
   Confirm portrait still fits and simultaneous buttons still work.
6. Reload and restart the control plane: the same browser cookie should retain
   the seat identity. Cookies are not copied to other browsers/devices.
7. Recheck `/client` and `/admin` for regressions. V2 cookies alone must not
   unlock existing APIs or Admin operations.
8. Alone in a game, click Leave Game: the session should enter `stopping`, then
   disappear when the host finishes graceful runtime/media cleanup. Immediately
   starting another game from the released seat must work. Repeat with two
   players and with a spectator: the game stays running for whoever remains,
   then stops when the final participant leaves. A pending join reservation is
   protected. Network/media disconnects alone retain reconnect grace.

After updating this fix, reload all browser clients. Sessions stranded by the
old disconnect-on-leave behavior are not bulk-stopped automatically; stop a known
orphan once through Admin, or rejoin and deliberately leave with the updated UI.

First iOS, LAN, and visual acceptance remain manual. Do not claim native-device
performance from desktop previews or simulated peers.

## Automated checks

```powershell
cargo test -p control-plane-server
cargo clippy -p control-plane-server --all-targets -- -D warnings -A clippy::too_many_arguments
node --test tools/test-client-v2.cjs tools/test-client-carousel.cjs tools/test-browser-input.cjs tools/test-browser-touch.cjs tools/test-browser-media.cjs tools/test-browser-media-lifecycle.cjs tools/test-browser-spectator.cjs tools/test-admin-asset-upload.cjs tools/test-admin-metadata-provider.cjs
```

The Clippy argument-count allowance covers pre-existing constructor/store APIs.
Security tests cover fail-closed config, source network/Host/Origin/request-header
checks, spoofed forwarded addresses, cookie signature/expiry/key rotation,
stable seats across restart/IP changes, admin/global-grant denials, response
token stripping, own-session access, and real-store cross-seat slot ownership.
Browser tests cover URL scoping, launch readiness/failure/timeout, duplicate
launch suppression, automatic media handoff and runtime button counts, plus
existing input, media lifecycle and spectator promotion regressions.
