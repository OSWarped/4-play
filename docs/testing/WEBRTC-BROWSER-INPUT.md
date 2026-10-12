# Browser gameplay input validation

The `/client` browser player now offers keyboard and standard gamepad controls
when opened for an occupied player slot owned by the requesting seat. The client
creates the `4play-input` data channel (unordered, no retransmissions). Complete
states are sent immediately on key changes and every 20 ms while connected.
Sequence numbers reject late states. Input stops when the page/player loses focus
and controls are neutralized on shutdown. The existing runtime timeout provides
another release safeguard after 250 ms without accepted input.

The control plane checks slot ownership and forwards the runtime input token only
to the publisher, using server-to-server headers. Neither the SDP answer nor the
input messages contain this token. The publisher fixes the authorized player
number and never accepts a browser-supplied session-stop flag. Spectator/producer
offers receive no input authorization. A 60-second input authorization is renewed
every 20 seconds while the page remains connected; renewal rechecks slot ownership.
Disconnect/release changes may take up to 60 seconds to revoke an already open
input channel if the old page ignores renewal failure. Stop/close sends neutral
input immediately; closing the player does not stop the entire MAME session.

The publisher remains experimental and currently has one WebRTC peer per session.
Opening another browser receiver replaces the previous peer. Simultaneous browser
players/spectators require a subsequent publisher fan-out implementation. The
existing UDP multiplayer/spectator receivers continue to be available.

## Automated checks

Windows or Linux:

```text
node --test tools/test-browser-input.cjs
python -B tools/test-webrtc-browser-input.py
cargo test -p control-plane-server -p control-protocol -p runtime-host-agent
```

Linux with GStreamer bindings:

```bash
python3 -B tools/test-webrtc-raw-publisher.py
python3 -B tools/test-webrtc-datachannel.py
```

The data-channel integration test creates two real WebRTC peers and verifies that
SCTP delivers a P2 diagonal/two-button combo and its release as 33-byte
authenticated UDP packets. It uses a temporary receiver, not an active MAME game.
Browser controller tests cover held directions, simultaneous buttons, focus loss,
hidden pages, gamepad mapping, and channel closure. API tests reject other-seat or
open-slot input, omit spectator authorization, and check private-header forwarding
and lease renewal. Relay tests check packet shape, sequence ordering, fixed slot,
lease expiry, stale peers, and reconnects.

## Deploy

```bash
cd ~/src/4-play
git pull --ff-only origin main
cargo build --release -p control-plane-server -p runtime-host-agent
sudo systemctl restart 4play-control-plane.service
```

A fresh session loads the new Python publisher. Restarting the host agent is not
needed to test this input path; restarting it can affect active games. For other
machines, install the publisher together with `tools/webrtc_browser_input.py` in
the same directory. Keep publisher signaling bound to loopback behind the control
plane; its private authorization headers assume trusted server-to-server traffic.
`appsrc` is now included in the host's media prerequisite check.

## Manual gameplay check

1. Stop the old test session and hard-refresh `/client` (Ctrl+F5).
2. Start a fresh game as a player, then choose **Open in browser player**.
3. Wait for **Controls ready** and click the game video to focus it.
4. Press **1** to insert a coin, then **2** to start.
5. Hold **W/A/S/D** to move. Test diagonals and **J + K** together in TMNT.
6. Six attack buttons are **J/K/L** and **M/,/.** for games such as Killer Instinct.
7. Change tabs or click a settings field while moving. Movement should stop.
8. Use **Fullscreen**, click the video, and repeat the controls check.
9. Play for more than one minute to exercise input lease renewal. Check that music
   stays clean and action/sound remain synchronized.

Standard gamepads use left stick/D-pad for movement, face buttons and shoulders
for six attacks, Back/Select for coin, and Start for start. Browser gamepad support
depends on browser permissions and may require a secure context; keyboard input
does not depend on gamepad availability.

If controls do not work, inspect the newest publisher log for
`Browser input channel authorized for player ...`, and the session runtime log for
`Seat input connected from 127.0.0.1:...`. The connection source stays stable across
peer replacement; after switching from a different input client, allow the runtime
input timeout to release that client's source lock.
