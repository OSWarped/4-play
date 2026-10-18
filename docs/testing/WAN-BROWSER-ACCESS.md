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

1. Coturn is installed and remains masked/inactive. Real loopback protocol tests
   passed for REST authentication, UDP/TCP clients, bounded UDP relay payload
   round trips, allocation release, and expired/wrong-secret rejection. These
   tests do not prove browser media, WAN NAT traversal, or latency. The installed
   Debian `4.6.1-2` package has unresolved security issues and must not be exposed;
   see the patched-build gate below. Real forced-relay media/input remains pending.
2. Coturn configuration must authenticate, bound allocations/bandwidth, disable
   CLI/admin and unsafe features, set correct external/private address mapping,
   and enforce peer destination restrictions. In particular do not create a TURN
   proxy into unrelated LAN services. Plan host-level egress port restrictions if
   the private MAME host must be an allowed peer; IP allowlisting alone is not a
   sufficient port restriction. No active coturn config is shipped yet.
3. Established WAN media now checks admission expiry/revocation through the
   existing background maintenance thread, independent of browser cooperation.
   The control plane sends only a domain-separated admission fingerprint to the
   private publisher; it never sends the browser credential. Publishers read the
   same private invitation DB read-only, cache at most one second, and fail closed
   on unavailable/unsafe storage. Typical closure is within two maintenance ticks;
   OS scheduling is not a hard real-time deadline. LAN peers do not use this check.
   Root setup must pass the same `FOURPLAY_WAN_TESTER_DATABASE` path to the host
   agent and control plane. Missing/invalid admission data blocks WAN signaling.
4. Delegated relay credentials expire after one hour. Automatic credential/ICE
   renewal is not implemented yet; use bounded experimental tests, not an
   unattended all-night session. Do not relax credential expiry as a workaround.
5. Generate/install private keys and environment files, rotate development API
   tokens, verify certificate renewal, and atomically switch the nginx staging
   site to the reviewed candidate only after all activation checks pass.
6. Only then open exact firewall/router ports and test cellular direct/forced
   relay, multiple testers, cleanup, responsiveness, and existing sites/LAN.

## Completed operator action: install the relay tool without enabling it

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

## Next operator action: prerequisites for a separately pinned patched build

Reference-host inspection after package installation found:

- `coturn 4.6.1-2`, masked and inactive.
- No patched candidate in the currently configured Debian trixie repositories.
- Debian marks this package vulnerable, including the IPv4-mapped IPv6 peer-ACL
  bypass [CVE-2026-27624](https://security-tracker.debian.org/tracker/CVE-2026-27624).
  Review the [complete coturn tracker](https://security-tracker.debian.org/tracker/source-package/coturn)
  again before activation; do not assume one fixed CVE is the entire security gate.
- The upstream `4.18.0` source tag resolves to commit
  `23c6c1d32a3d2b21a56cee65d3203bcc4ea82d2b`. That exact commit is pinned in
  `deploy/systemd/build-private-coturn.py`; changed tag contents are rejected.
- Compiler, make, git, pkg-config and OpenSSL development files are present.
  libevent and SQLite development files are missing.

Run on Linux:

```bash
sudo apt-get install --no-install-recommends libevent-dev libsqlite3-dev
```

Then the agent can run, without root:

```bash
cd ~/src/4-play
python3 deploy/systemd/build-private-coturn.py
python3 deploy/systemd/build-private-coturn.py --build
```

The builder uses a new private temporary directory, verifies the source commit,
removes service secrets/compiler hooks from the build environment, and builds with
two jobs. Optional database backends, metrics, systemd integration and SCTP are
disabled; OpenSSL and SQLite remain available. It never runs `make install`, starts
a daemon, replaces the Debian binary, or changes existing applications. A provenance
receipt and the build tree remain for review; `/tmp` is not a production install.
A reviewed persistent deployment is a separate step after validation.

Test the resulting binary using its printed absolute path:

```bash
python3 tools/test-coturn-loopback.py --turnserver /ABSOLUTE/BUILD/PATH/bin/turnserver
```

The loopback test uses its own mode-0600 config/random credentials, a random
127.0.0.1 listener, and fixture relay ports 62000–62031, separate from production
pools. It stops its own process and removes only its own temporary artifacts.
Never copy its `allow-loopback-peers` setting into public configuration.
Until the patched binary and public security gates are validated, keep coturn
masked and the public website on staging 503. No router rules yet.

## Patched build and real forced-relay evidence (reference host)

The pinned source was built successfully as the ordinary service user at:

`/tmp/4play-coturn-4.18.0-2dm5_dga/source/bin/turnserver`

Its SHA-256 is:

`afca6458e0bd736bb485eb6f934732fbd2cc6b51cdb6538b046ef8cda38b9b86`

This fingerprint identifies this specific compiled artifact, not every rebuild.
The build receipt records upstream version/commit; no system binary was replaced.
Checks passed:

- Patched coturn REST authentication, UDP/TCP relay clients, payload round trips,
  expired/wrong-secret rejection, bounded relay range, explicit allocation release.
- Upstream `turnutils_rfc5769check` conformance checks.
- Real GStreamer sender and receiver, forced-relay ICE on **both** peers: decoded
  audio/video and SCTP controller press/release, not just a STUN allocation.
- A simultaneous LAN spectator retained host-only ICE and continued decoding.
- A second relayed spectator decoded audio/video. Revoking the player's admission
  closed only that player; expiring the spectator's admission closed only that
  spectator; the LAN spectator continued.

The loopback media fixture substitutes its local endpoint and maps a copy of
loopback candidate addresses/ranges only for validation. It does not change
negotiated SDP addresses or the production WAN validator. It reuses the real raw
publisher, encoders, REST credentials, input bridge, and peer ownership. This is
**not** proof of real internet routing, Safari/browser interoperability, or WAN
latency. Those tests still follow deployment/network activation.

Known cleanup warning: the installed libnice/GStreamer stack reports live TURN
refreshes during peer disposal. Investigate explicit asynchronous allocation
cleanup before public activation; the loopback fixture stops its private relay,
so it does not leave a public allocation/server behind.

## Next operator action: protect the tested binary outside /tmp

Run only after the updated installer is present in the Linux checkout:

```bash
cd ~/src/4-play
sudo python3 deploy/systemd/install-private-coturn.py \
  --build-directory /tmp/4play-coturn-4.18.0-2dm5_dga \
  --sha256 afca6458e0bd736bb485eb6f934732fbd2cc6b51cdb6538b046ef8cda38b9b86 \
  --apply
```

The installer validates the receipt/pinned commit and hashes the exact bytes it
copies. It never executes the user-built binary as root. It installs exclusively
at `/opt/4play-coturn/4.18.0/turnserver`, requires root-owned/non-writable directory
ancestors, refuses conflicting existing binaries or symlinks, and is idempotent
for matching protected bytes. Debian's binary, all services, environment files,
firewalls and nginx remain unchanged. If the build directory no longer exists,
rebuild/retest and obtain its new fingerprint; do not bypass verification.

This is binary staging only. Keep `coturn.service` masked. A dedicated least-
privilege service and destination/port egress restrictions are still required.

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
