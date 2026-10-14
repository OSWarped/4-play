# Browser IP metadata for WebRTC signaling

## What changed

For direct access to the control plane, each request now gets its client's actual
TCP peer IP through Axum connection metadata. The control plane sends that address
to the same-host publisher using `x-4play-browser-ip`, together with the existing
private input authorization when applicable. No connection secrets or new browser
configuration fields are introduced, and browser-supplied address headers are
ignored rather than copied upstream.

The raw-session and test-pattern HTTP publishers accept this private header only
when the signaling TCP caller is loopback. They validate a literal non-loopback
unicast IP and normalize IPv4-mapped IPv6. Direct LAN callers cannot select another
client IP through the header. Invalid trusted metadata fails instead of silently
falling back to the wrong PC. No browser mDNS candidate is rewritten to the proxy's
`127.0.0.1` by default.

`FOURPLAY_WEBRTC_BROWSER_HOST_IP` is now only a legacy/manual fallback when the
loopback request has no trusted client metadata. A configured `192.168.20.10` does
not override the automatic `192.168.0.186` on normal direct browser signaling.
New installation examples leave it blank. Existing configurations do not need an
edit to use the new automatic path.

## Supported topology and limitations

- Browser connects directly to `http://192.168.20.68:8080`.
- Control plane and publisher run on the same host; publisher HTTP binds loopback.
- Routed subnets may work if routing and UDP ACL/firewall rules allow them; this
  implementation does not itself establish that connectivity.
- Reverse proxies are not automatically trusted. A loopback proxy cannot provide
  a useful non-loopback browser TCP address through this mechanism; explicit
  proxy trust configuration is future work. Do not enable unconditional trust of
  `X-Forwarded-For` or expose publishers publicly to bypass this restriction.
- Remote runtime hosts need an explicit signaling-proxy trust design rather than
  accepting the private header from every network caller.
- NAT may change the observed address or port and requires ICE/STUN/TURN support
  beyond this LAN candidate-rewrite bridge. Neither a browser Media IP setting nor
  a TCP forwarding header can solve UDP NAT traversal by itself.

## Automated validation

```powershell
cargo test -p control-plane-server -p control-protocol -p runtime-host-agent
python tools/test-webrtc-client-address.py
```

The Rust regression exercises actual API routing with trusted connection metadata
for `192.168.20.10`, `192.168.0.186`, mapped IPv4, and absent/loopback metadata. It
also injects forged private/standard forwarding headers and verifies the publisher
receives only transport-derived addresses. The Python suite includes actual local
HTTP requests against the shipped raw publisher handler without needing GI, plus
address validation and legacy fallback checks. Existing Linux WebRTC media/input
integration tests remain required; cross-subnet acceptance is still manual.

## Activate

The update is pulled and the control-plane binary built during reference-host
deployment. Stop existing sessions through Admin so new publishers import the
updated Python helper. Then on Linux:

```bash
sudo systemctl restart 4play-control-plane.service
```

Refresh the browser. For `/webrtc-test` using the standalone service, also restart
`4play-webrtc-spike-publisher.service` between tests. Do not restart a publisher
while someone is playing. No environment/firewall edits are made by this update.

## Later cross-subnet check

1. Use a distinct browser profile/seat ID on the other PC. Enter that PC's Media IP
   (currently `192.168.0.186`) and open the control-plane URL directly.
2. Launch a new session and open browser media. In its `.webrtc.log`, confirm the
   mDNS replacement uses the new PC address instead of `192.168.20.10`:

   ```bash
   grep 'Replaced.*mDNS' /tmp/4play/host-agent/ACTUAL-SESSION-ID.webrtc.log
   ```

   Replace `ACTUAL-SESSION-ID` with the ID displayed by your session; do not paste
   literal angle-bracket placeholders into shell redirections.
3. If ICE fails, capture while reconnecting:

   ```bash
   sudo timeout 20 tcpdump -ni enp0s31f6 'host 192.168.0.186 and udp'
   sudo ufw status numbered
   ```

   Packet arrival does not by itself prove UFW accepted it. No packets suggest a
   client/network path problem, but further router/client evidence is needed.
4. TCP 8080 is signaling, not media. WebRTC still uses dynamic UDP ports; the
   existing `41000`/`42000` rules are not a complete WebRTC media policy. Bounded
   ICE port configuration and corresponding firewall guidance are the next task.
5. The earlier one-PC UDP allow was diagnostic only. If it was added, remove it
   after testing once an appropriate bounded rule is in place:

   ```bash
   sudo ufw delete allow from 192.168.0.186 to any proto udp
   ```

No claim of cross-subnet success should be recorded until real playback and input
are observed on that PC, including simultaneous access from the original PC.
