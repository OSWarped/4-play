# Browser client recovery validation

The client now shows receiver diagnostics and provides **Reconnect media**. This
does not allocate another session, player slot, spectator grant, or media port.
It repeats authorized signaling for the existing view. The owner confirmed this
recovery increment works. The subsequent multi-peer publisher now permits
independent viewers; follow [multi-peer validation](WEBRTC-MULTI-PEER.md) for that
additional acceptance test. UDP/FFplay remains unchanged.

## Automated checks

From the repository root:

```powershell
node --test tools/test-browser-input.cjs tools/test-browser-media.cjs tools/test-browser-media-lifecycle.cjs
node --check control-plane/server/static/client/app.js
cargo test -p control-plane-server -p control-protocol -p runtime-host-agent
```

The tests cover held input and Escape; frame-rate/jitter/loss/RTT reporting;
missing stats; stalled and recovered decoding; no overlapping stats requests;
teardown while stats or signaling are pending; disconnected input neutralization;
reconnect without slot mutation; failed offers retaining Leave/Retry; and stale
peer callbacks being ignored. Browser/GStreamer integration and perceived latency
are not established by these unit tests.

## Deploy

On the Linux server:

```bash
cd ~/src/4-play
git pull --ff-only origin main
/home/blake/.cargo/bin/cargo build --release -p control-plane-server
sudo systemctl restart 4play-control-plane.service
```

Restarting the control plane briefly interrupts API access. Do this between play
tests. No host-agent/publisher restart or firewall change is required for this
client-only update. Refresh `/client` with Ctrl+F5 after restarting.

## Manual reference-PC test

1. Start one game, open its browser player, and verify sound plus held directions
   and simultaneous actions. Receiver decoded frames should advance; FPS is a
   sampled diagnostic, not an input-to-photon measurement.
2. Select Reconnect media. Expect a brief media interruption, then resumed media
   and controls in the same player slot. No extra session or spectator grant
   should appear. The new peer's counters may start over.
3. In windowed mode, press Escape with the game focused. Confirm controls release,
   media closes, your player slot disconnects, and library browsing returns.
4. Rejoin and enter fullscreen. The first Escape exits browser fullscreen; the
   second leaves the game. Confirm another player's session is never stopped.
5. Temporarily disconnect the reference PC's network during play. Controls must
   be released on connection/ICE disconnect, with the runtime timeout remaining
   a backstop. Reconnect the network and use Reconnect media if it does not
   recover. Reconnect may fail if the seat/grant has expired; rejoin normally.
6. If a peer connects but no video advances for three seconds, expect the stalled
   warning. Missing receiver stats are explicitly reported instead of inventing
   frame rates. Jitter and RTT are network metrics, not total gameplay latency.

Record real-play results before marking recovery or Escape manually accepted.
Other PCs/subnets remain deferred, including the fixed browser-IP workaround.
