# Spectator actions and joining from the browser video

## Behavior

While spectating, the actions menu slides up from the bottom of the video when
the user presses a key, moves/presses a standard gamepad, or moves the pointer.
It slides away after 3.5 seconds of inactivity. Pointer hover, deliberate keyboard
focus in the menu, and a pending join keep it visible for accessibility. Animation
is disabled when the browser reports a reduced-motion preference.

The menu offers open positions (plus this seat's reserved/disconnected positions),
fullscreen, reconnect, and leave. Availability refreshes every five seconds, but
the control plane's atomic reservation is authoritative. Navigation is via normal
Tab/Enter, arrows/WASD, mouse, or joystick/D-pad and gamepad A. A button that wakes
a hidden menu cannot simultaneously activate a join; held buttons do not repeat
confirmation. Spectator interaction never forwards game controller state.

Joining uses the active view's original seat ID, even if settings were edited.
It reserves/connects the selected player before starting player signaling. A
reservation conflict leaves spectator media running. Cancellation or a connection
failure disconnects only the acquired slot; cleanup failures are reported for
operator recovery. A successful handoff closes the spectator view, opens player
media/input without another manual button click, and releases its spectator grant.
If media negotiation fails after ownership changes, the player view retains
Reconnect/Leave rather than silently abandoning the occupied slot. If releasing
the old spectator grant fails, a warning is shown and its expiry remains a backstop.

At the WebRTC peer limit, promotion replaces only the server-identified old
spectator view. It does not evict another viewer or player. UDP/FFplay remains
unchanged; cross-subnet network acceptance remains deferred. Per-request IP
metadata is covered in [client address validation](WEBRTC-CLIENT-ADDRESS.md).

## Automated validation

From the Windows repository root:

```powershell
node --test tools/test-browser-input.cjs tools/test-browser-media.cjs tools/test-browser-media-lifecycle.cjs tools/test-browser-spectator.cjs
cargo test -p control-plane-server
python tools/test-webrtc-peer-pool.py
```

From the Linux repository root:

```bash
python3 tools/test-webrtc-multi-peer.py
```

Node checks exercise the shipped menu controller and transition flow, including
auto-hide, focus retention, gamepad edge detection, eligible slots, reservation
conflicts, cancellation cleanup, identity preservation, authorized input ordering,
and spectator-grant release. Rust checks cover public static assets and the private
promotion identity sent to the publisher. The real Linux test promotes a spectator
to P3 at a full three-peer limit while P1/P2 continue decoding, then reconnects P2
and verifies isolated input. Actual browser styling/gamepad feel is not established
by these automated tests.

## Activate on the reference host

The update is pulled and the control-plane release binary built during deployment.
Stop existing test sessions using the Admin console so subsequent games use the
new Python peer-pool code. Then on Linux:

```bash
sudo systemctl restart 4play-control-plane.service
```

No host-agent binary/config or firewall changes are required. Refresh `/client`
with Ctrl+F5. Browser profiles must have distinct seat IDs; ordinary tabs share
localStorage settings. Use the original reference PC for this test.

## Manual acceptance

1. Profile A (`windows-seat-1`): start TMNT or another multiplayer game and play P1.
2. Profile B (`windows-seat-2`): spectate the same session and open the browser
   player. Watch media for at least a minute. Confirm P1 is not interrupted.
3. Click the video and move the pointer away. With focus on video, confirm the
   overlay tucks away; press a key or move a gamepad to bring it back. Check the
   same behavior in fullscreen and with reduced-motion enabled if available.
4. Navigate menu actions using arrows/Tab and gamepad D-pad/A. Verify waking or
   holding a button never joins by accident and spectator attack keys do not
   move P1. Check that keyboard-focused menu actions stay visible.
5. Select Join P2. Media should reconnect automatically as player 2. Insert
   coin/start and verify independent gameplay. Confirm the spectator count drops
   and no new MAME session appears. P1 must continue through the handoff.
6. Have another profile claim an advertised slot first; the losing reservation
   must show an error while continuing to spectate. Availability will refresh.
7. Leave P2 with Escape, rejoin/spectate, and exercise reconnect/leave controls.
   Check normal and fullscreen modes. Leaving must not stop the shared session.
8. Record smoothness, audio sync, and any growing delay during a longer shared
   game. Keep multi-peer/menu acceptance pending until these observations pass.
