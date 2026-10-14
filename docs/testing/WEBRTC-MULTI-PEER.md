# Independent browser players and spectators

## Implementation and limits

The opt-in MAME WebRTC publisher uses one frame-aligned FIFO reader per raw stream
and independent VP8/Opus/WebRTC pipelines per browser. Bounded leaky queues remain
unchanged. This does not replace UDP/FFplay or alter MAME's original FIFO readers.

- Default: eight browser peers per MAME session (players plus spectators).
- Optional `FOURPLAY_WEBRTC_MAX_PEERS=8` in `/etc/4play/4play.env`; range 1-32.
  The Python publisher inherits this environment. Do not raise the cap without
  measuring CPU and perceived latency: this version encodes once per viewer.
- Capacity rejects a new viewer without disconnecting existing peers.
- Reconnect replaces only its player slot or spectator seat/grant. The control
  plane constructs the spectator key; browser-supplied forwarding headers are
  not passed through to the publisher.
- Each player slot keeps a stable loopback UDP source and sequence across
  reconnects. Stale peer callbacks cannot revoke a successor's input authority.
- Spectators cannot send controller state, even if they request an input channel.
- Failed/closed peers are removed on the next maintenance sweep. Connecting peers
  have a 30-second setup deadline; disconnected peers have a 15-second recovery
  grace. Player peers with expired input authorization are also removed. These
  deadlines start after the corresponding local peer state is observed.
- All peers and relay sockets are closed when the publisher/session stops.
- Keep publisher HTTP listeners loopback-only: private input headers are trusted
  server-to-server authorization, not a public authentication mechanism.
- Other-machine/subnet playback acceptance remains deferred. Per-request browser
  IP metadata now overrides the old fixed-PC fallback, but UDP firewall rules,
  routing/ACLs, and NAT must still allow the actual WebRTC connection. See
  [client address validation](WEBRTC-CLIENT-ADDRESS.md).

## Automated tests

From the Linux repository root:

```bash
python3 tools/test-webrtc-peer-pool.py
python3 tools/test-webrtc-browser-input.py
python3 tools/test-webrtc-raw-publisher.py
python3 tools/test-webrtc-datachannel.py
python3 tools/test-webrtc-multi-peer.py
```

The last test creates local GStreamer peers, decodes generated video and audio
concurrently for P1/P2/a spectator, rejects spectator input, verifies authenticated
P1/P2 UDP states, reconnects P2 and the spectator at capacity, and confirms P1
survives the changes plus malformed signaling. It uses throwaway files/local
ephemeral ports; no active game or control-plane service is required. These
tests passed on the Linux reference host; actual browser/MAME acceptance is pending.

The registry/input/framing/timing tests also run on Windows, with the GStreamer
encoder check skipped there. Existing Rust and Node regression suites remain
required. Receiver stats are not a substitute for perceived latency validation.

## Deployment

The changes were pulled and the control-plane binary built on the reference host.
To activate, end the current test games first, then on Linux:

```bash
sudo systemctl restart 4play-control-plane.service
```

New MAME sessions start the updated Python publisher. Existing publishers keep
their old code until their session is stopped; do not kill a live publisher just
to update it. No host-agent binary change or firewall change is required. Refresh
the browser with Ctrl+F5.

## Same-PC manual test

Use separate browser profiles or one normal window and one Incognito/InPrivate
window. Ordinary tabs share localStorage seat settings and can accidentally use
the same identity. Enter the existing seat API token in each profile; never post
private connection grants or tokens in validation screenshots/logs.

1. Profile A: configure `windows-seat-1`, destination `192.168.20.10`. Start a
   multiplayer game (Aliens or TMNT), open the browser player, insert coin/start,
   and confirm P1 movement and sound.
2. Profile B: configure `windows-seat-2`, same destination IP. Join P2 of A's
   active session and open its browser player. P1's video must continue. Focus
   each game view in turn and confirm it controls only its assigned player.
   A single keyboard cannot independently focus two windows simultaneously.
3. Use a third isolated profile as `windows-seat-3` to spectate that session.
   Confirm all three show media and spectator keyboard presses do not control
   either player or reserve a slot.
4. Reconnect the spectator, then P2. P1's video and controls must remain usable.
   Reconnect must not create another MAME session or spectator grant.
5. Leave P2 with Escape. P1 and spectator media must continue. Rejoin P2 and
   verify it works again. Leave the spectator and confirm both players continue.
6. Play for at least ten minutes and note sound sync, input feel, decoded FPS,
   dropped frames, and whether delay grows. On Linux, use `top` to watch the
   publisher's CPU/memory. Record results before accepting multi-peer gameplay.

Next UI work after acceptance: animated spectator actions and joining an open
player slot directly from the spectator view.
