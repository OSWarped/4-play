# Bounded WebRTC UDP ports and firewall validation

## Policy

Both the real-session and test-pattern publishers use this shared host ICE pool:

```text
FOURPLAY_WEBRTC_ICE_UDP_PORT_START=50000
FOURPLAY_WEBRTC_ICE_UDP_PORT_COUNT=1000
```

Existing environment files need no edit for these defaults. New installation
examples include them. Manual publishers can override with `--ice-udp-port-start`
and `--ice-udp-port-count`. Valid ranges have a positive count, stay within
1-65535, and do not overlap the configured legacy input/media port pools.

The pool is shared by all peer/session publishers on the host. libnice selects
available ports within it; there is no fixed one-port-per-seat assignment or
promise that a particular pool size accommodates every interface/peer count.
Video, audio, and browser input use the same negotiated bundled ICE transport.
Keep enough headroom for concurrent viewers and interface candidates; do not
increase the peer limit without resource and allocation measurements.

ICE-TCP and automatic UPnP mapping are disabled in this reference LAN path. The SDP answer must contain usable
UDP host candidates inside the pool, or signaling fails explicitly. No unbounded
UDP/TCP fallback, STUN/TURN, or public-internet NAT traversal is introduced. This
is intentional: networks that require TCP fallback need a separate transport
design rather than quietly opening additional ports. UDP/FFplay remains unchanged.

GStreamer's documented [ICE range properties](https://gstreamer.freedesktop.org/documentation/webrtclib/gstwebrtcice.html)
are applied before gathering. The installed reference stack is GStreamer 1.26.2;
unsupported ICE properties fail the readiness probe. Testing also exposed a
default-agent floating-reference lifetime issue. The narrowly guarded compatibility
path restores the native bin owner only when the agent has one reference; the bin
releases that owner during normal disposal. See the default construction/disposal
in [the 1.26.2 source](https://github.com/GStreamer/gstreamer/blob/1.26.2/subprojects/gst-plugins-bad/ext/webrtc/gstwebrtcbin.c).
Integration tests treat critical messages as fatal and verify an old ICE object
is actually destroyed after reconnect, not merely removed from the peer registry.

Only advertised/local ICE candidate ports are bounded by this policy. Internal
loopback UDP input relay sockets and the legacy outbound FFmpeg stream are not
assigned from it and do not require exposing their ephemeral ports to the LAN.

## Generate a plan — no firewall mutations

On Linux, from the repository root:

```bash
sudo python3 tools/webrtc-firewall-plan.py \
  --env-file /etc/4play/4play.env \
  --source 192.168.20.0/24 \
  --source 192.168.0.186
```

Root is needed only to read the private environment file. The planner selects
nonsecret literal port settings, never sources/evaluates shell code, and prints
UFW commands plus nftables examples. It never applies either. Sources can be
repeated IPv4/IPv6 hosts or subnets; unrestricted `/0` rules are rejected. Use
one firewall manager and your actual nftables table/chain if selecting those
examples. The old Phase 3 planner covers legacy UDP clients, not this ICE policy.

For the current defaults, the additional UFW media rules would be:

```bash
sudo ufw allow from 192.168.20.0/24 to any port 50000:50999 proto udp comment '4-Play WebRTC ICE'
sudo ufw allow from 192.168.0.186 to any port 50000:50999 proto udp comment '4-Play WebRTC ICE'
```

Review these before applying. TCP 8080 rules already exist for the reference
subnets. Do not expose the loopback publisher signaling ports (`18080`/`43000`)
to the LAN. Browser input travels through WebRTC; direct `42000` network access
is still for the legacy terminal/native UDP input path.

If the temporary all-UDP rule for the other PC was previously added, replace it
with the bounded rule, then remove only that temporary rule:

```bash
sudo ufw delete allow from 192.168.0.186 to any proto udp
sudo ufw status numbered
```

No firewall changes have been applied automatically. These rules do not change
router ACLs, Windows firewall, DHCP addressing, or NAT behavior. Configure a stable
client address or use an explicitly trusted subnet if a host-specific rule must
survive DHCP changes. IPv6 needs its own applicable source/routing policy.

## Activation

The code is pulled and release binaries built during deployment. Before activating,
stop old test sessions in Admin and review/apply the appropriate firewall plan.
New sessions use the new Python policy; old publishers retain their old code.
Activate the enhanced agent readiness check (and earlier pending client updates):

```bash
sudo systemctl restart 4play-control-plane.service 4play-runtime-host-agent.service
```

If using the standalone `/webrtc-test` publisher service, restart
`4play-webrtc-spike-publisher.service` between tests as well. Don't interrupt a
live game to update its publisher. Refresh `/client` with Ctrl+F5 afterward.

## Automated checks

Portable policy/planner tests:

```powershell
python tools/test-webrtc-ice-ports.py
python tools/test-webrtc-client-address.py
python tools/test-webrtc-peer-pool.py
cargo test -p control-plane-server -p control-protocol -p runtime-host-agent -- --test-threads=1
```

On Linux, additionally:

```bash
bash deploy/systemd/check-browser-media-prereqs.sh
G_DEBUG=fatal-criticals python3 tools/webrtc_ice_ports.py --check
G_DEBUG=fatal-criticals python3 tools/test-webrtc-multi-peer.py
G_DEBUG=fatal-criticals python3 tools/test-webrtc-datachannel.py
G_DEBUG=fatal-criticals python3 tools/test-webrtc-ice-linux.py
```

The media tests decode real generated VP8/Opus and exercise authenticated input,
reconnect, and spectator promotion. They verify nondefault bounded pools
(`51000-51099`/`52000-52099`), no TCP candidates, native-object cleanup, and a pool
whose sole port is occupied failing without fallback. They use only temporary
files, local peers, and unprivileged sockets; no running MAME session or live
firewall is touched. Host input/media sockets remain distinct from ICE sockets.
The serial Rust command avoids existing short wall-clock expiry tests being
affected by unrelated parallel test scheduling.

## Reference network acceptance

On 2026-10-14 the project owner confirmed successful joining-player and spectator
playback from `192.168.0.186`. This accepts the tested routed LAN topology, not
arbitrary router/NAT/proxy environments. Use the following checklist for new
installations and regressions; longer gameplay/resource validation is separate.

1. Start a new game on the original PC; verify media, held controls, audio sync,
   spectator joining, reconnect, and continued playback for other seats.
2. On `192.168.0.186`, use a distinct seat/profile and your own Media IP. Access
   the control-plane URL directly, start/join a game, and verify actual playback.
3. Check the publisher's startup/health log reports `ice_udp=50000-50999` (or your
   configured pool). Saved SDP answers must contain UDP host candidates within it.
4. If connection fails, capture while reconnecting:

   ```bash
   sudo timeout 20 tcpdump -ni enp0s31f6 'host 192.168.0.186 and udp portrange 50000-50999'
   sudo ufw status numbered
   ```

   Change the capture range if configured differently. Packet arrival does not
   prove UFW accepted it; no arrival does not identify the router as the cause
   without additional client/router evidence.
5. Confirm simultaneous original-PC and other-PC access, then perform a longer
   shared-game soak. Record resource use, audio sync, and perceived input/video
   delay. Automated local tests are not routed-subnet or latency acceptance.

Next steps after acceptance: longer stability/resource testing, then in-page
Admin/Producer capture; explicit reverse-proxy/NAT trust and STUN/TURN remain future
deployment work.
