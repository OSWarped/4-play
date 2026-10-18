# WAN browser admission and experimental media

Status: implementation behind explicit opt-in settings. **The public hostname
must remain on the staging 503 site.** Invitations, proxy trust, resource limits,
and an experimental WAN ICE profile are implemented; TURN installation, real
relay integration, and external acceptance are still required before activation.

## Admission and trusted edge

Set all three variables together only during the later operator setup:

| Control-plane setting | Meaning |
| --- | --- |
| `FOURPLAY_WAN_TESTER_ORIGIN` | `https://arcade.blakemilam.com` (exact origin, no slash/path) |
| `FOURPLAY_WAN_EDGE_KEY` | Separate cryptographically random 64-hex key shared only with nginx |
| `FOURPLAY_WAN_TESTER_DATABASE` | Existing private invitation database, run as the service user |

Defaults: disabled. Partial configuration fails startup. The public origin must
not be added to LAN enrollment origins, and loopback/all-internet networks must
not be added to the LAN enrollment CIDRs.

Public enrollment requires actual TCP peer **127.0.0.1**, exact configured Host,
a matching private edge key, and a single valid `X-4Play-Client-IP` supplied by
nginx. Other forwarding headers are ignored. nginx overwrites those headers;
direct remote callers cannot supply them to bypass enrollment. Origin, client
header and fetch-site checks protect writes; form submissions require JSON.

- `/client-v2/tester-access`: GET invitation form; POST code in the request body.
- At most five enrollment attempts per IP per minute, sixty globally per minute.
- A valid existing cookie makes enrollment retries idempotent.
- `__Secure-fourplay_tester` cookie: HttpOnly, Secure, SameSite=Strict,
  Path=/client-v2, expires no later than the stored admission.
- V4 bootstrap redirects to the form only for `tester_access_required`.
- Every scoped request rechecks database authorization; revoked/expired cookies
  fail even if they have a valid format. A LAN cookie is not a WAN admission.
- The scoped proxy reconstructs headers rather than copying private headers,
  strips service tokens from responses, and forces seat identity/client address.
- Public sessions use the existing player-slot ownership checks. Public creation
  is additionally denied when three nonterminal sessions already exist (including
  LAN sessions); public spectator allocation allows eight grants globally and one
  per seat. Same-session spectator reconnects renew/reuse the grant. These limits
  run inside allocation transactions, not in the UI. LAN allocation is unchanged.

## ICE configuration

`GET /client-v2/webrtc-config` requires the scoped admission. LAN returns the
existing `iceServers: []`. WAN returns per-seat derived one-hour coturn REST
credentials plus STUN and TURN/UDP + TURN/TCP URLs on port 3478. Neither the
invitation credential nor the persistent TURN shared secret is returned to JS.
The delegated TURN credentials necessarily enter RTCPeerConnection configuration;
do not treat them as persistent service API keys or log them.

Settings on **control plane and runtime host agent**, not in browser configuration:

- `FOURPLAY_WAN_TURN_HOST=arcade.blakemilam.com`
- `FOURPLAY_WAN_TURN_SECRET=<separate random 64-hex coturn REST shared secret>`
- `FOURPLAY_WAN_PUBLIC_IP=45.24.46.155` on the publisher; confirm current WAN IP.

The control plane injects a private per-request WAN profile marker through a
trusted in-process extension. External headers cannot choose this profile. Raw
publishers accept it only from a loopback signaling caller. Each viewer has its
own profile: a WAN viewer does not switch other viewers to internet ICE.

WAN peers configure GStreamer's STUN/TURN support and do **not** rewrite browser
mDNS addresses to a TCP peer's public address. WAN SDP waits for complete ICE
gathering and requires a relay candidate matching the configured public IP and
bounded relay range. Candidate checks retain bounded host ports; server-reflexive
candidates must have a related bounded local port. LAN's host-only validator is
unchanged. No new media queues, retransmit buffering or encoder parameters are
introduced by this transport selection.

Proposed separate port pools (not yet forwarded/opened):

| Purpose | Ports |
| --- | --- |
| Existing HTTPS + signaling | TCP 443, existing TCP 80 for ACME/redirect |
| STUN/TURN listener | UDP/TCP 3478 |
| Existing bounded publisher ICE | UDP 50000–50999 |
| Proposed coturn relay allocations | UDP 52000–52199 |

TURN/TCP carries a UDP relay over a TCP connection to coturn; it does not enable
unbounded TCP ICE listeners on the publisher. TLS TURN/443 is not implemented by
this first candidate: nginx already owns 443. Networks blocking 3478 may require
a later TURN/TLS endpoint/host. Do not claim universal connectivity.

## Not yet complete / activation blockers

1. Install coturn safely without starting a default service, inspect the installed
   version/options, and validate one-hour credentials plus a real forced-relay
   media/input peer. No real TURN allocation has been tested by the pure tests.
2. Coturn configuration must authenticate, bound allocations/bandwidth, disable
   CLI/admin and unsafe features, set correct external/private address mapping,
   and enforce peer destination restrictions. In particular do not create a TURN
   proxy into unrelated LAN services. Plan host-level egress port restrictions if
   the private MAME host must be an allowed peer; IP allowlisting alone is not a
   sufficient port restriction. No active coturn config is shipped yet.
3. Current player input revocation is bounded by its existing short input lease.
   Spectator media is not forcibly disconnected by invitation revocation. Add
   server-enforced viewer admission expiry/revocation handling before activation.
   Revocation currently rejects subsequent HTTP authorization, not arbitrary
   already-established DTLS media connections.
4. Delegated relay credentials expire after one hour. Automatic credential/ICE
   renewal is not implemented yet; use bounded experimental tests, not an
   unattended all-night session. Do not relax credential expiry as a workaround.
5. Generate/install private keys and environment files, rotate development API
   tokens, verify certificate renewal, and atomically switch the nginx staging
   site to the reviewed candidate only after all activation checks pass.
6. Only then open exact firewall/router ports and test cellular direct/forced
   relay, multiple testers, cleanup, responsiveness, and existing sites/LAN.

## Next operator action: install the missing relay tool without enabling it

On the reference server coturn was not installed. Run these one at a time:

```bash
sudo systemctl mask coturn.service
sudo apt-get update
sudo apt-get install --no-install-recommends coturn
systemctl is-enabled coturn.service
```

The final result should be `masked` (a nonzero exit code from `is-enabled` is
normal for a masked service). Do not unmask/start it and do not add router rules
yet. Masking first prevents package installation from starting an unreviewed
default relay. Stop if a command fails or an existing unrelated coturn service
is discovered. There is no need to restart the 4-Play services for this step.

## Regression commands

```bash
cargo test -p control-plane-server
cargo clippy -p control-plane-server --all-targets -- -D warnings -A clippy::too_many_arguments
node --test tools/test-wan-tester-ui.cjs
python3 tools/test-webrtc-wan.py
python3 tools/test-wan-nginx.py
python3 tools/test-webrtc-client-address.py
python3 tools/test-webrtc-raw-publisher.py
python3 tools/test-webrtc-multi-peer.py
```

nginx test uses throwaway self-signed TLS, unprivileged loopback listeners and a
mock upstream, never the live service/configuration. It checks route exclusions
and actual header replacement, not merely config text. Other media tests use
temporary local peers, not production games/ports.

Candidate configuration: [nginx allowlist](../../deploy/nginx/arcade-public.conf.example).
It is deliberately not installed/enabled. Future setup must create the private
edge-key snippet with restrictive permissions; never put actual keys in Git.

Protocol references: [GStreamer webrtcbin STUN/TURN](https://gstreamer.freedesktop.org/documentation/webrtc/index.html),
[coturn configuration](https://github.com/coturn/coturn/blob/master/examples/etc/turnserver.conf),
[TURN overview](https://webrtc.org/getting-started/turn-server).
