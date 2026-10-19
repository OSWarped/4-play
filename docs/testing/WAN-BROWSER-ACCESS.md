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

Explicit WAN cleanup now uses libnice `close_async` before pipeline disposal.
Input is revoked immediately; cleanup runs in the default GLib context without
blocking the peer pool/media feeder, retaining the native owners until completion.
A five-second timeout disposes local resources and reports remote release as
unconfirmed (never as success). Shutdown drains callbacks for up to six seconds.
LAN peers retain immediate teardown. No encoder or playback buffer was changed.

The strengthened real relay fixture observes coturn-owned UDP sockets via `/proc`:
it confirms live allocations exist, checks revoked/expired publisher peers complete
cleanup, then checks **all fixture relay sockets disappear while coturn is still
running**, before stopping that private relay. This test passes on the reference
host, without the prior live-refresh/disposal warnings. A zero-lifetime TURN
refresh can require a short native teardown grace period; the assertion polls
with a bound rather than assuming synchronous socket removal.

The owner installed Debian's `gir1.2-nice-0.1` binding on the reference host.
Without it the agent appears as a generic GObject without `close_async`. The
cleanup helper registers this GI namespace before the first agent is constructed;
loading it later also leaves cached wrappers without the required methods. On a
new host, install it and run the opt-in WAN prerequisite check:

```bash
sudo apt-get install --no-install-recommends gir1.2-nice-0.1
bash deploy/systemd/check-browser-media-prereqs.sh --wan
```

This check opens no listener and modifies no services. The default check remains
LAN-only and does not require the new binding. A successful dependency check is
not proof of relay cleanup: real allocation-release tests must also pass.
The upstream lifecycle requirement is
documented in [libnice close_async](https://libnice.freedesktop.org/libnice/NiceAgent.html#nice-agent-close-async).

## Completed operator action: protect the tested binary outside /tmp

The owner completed this installation, and read-only verification confirmed the
installed binary is root:root mode 0755. The command below is retained for new
hosts/reinstallation; it need not be repeated on the reference host.

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

## Next operator action: isolated kernel egress validation (not installation)

The candidate renderer `deploy/systemd/private_turn_bundle.py` produces a separate
`4play-turn.service`, its required `4play-turn-egress.service`, a private systemd
credential config, and a dedicated `inet fourplay_turn` nft table. It does not
install or start anything. The proposed relay runs as the existing `turnserver`
account with empty capabilities, read-only system files, private temporary/device
directories, and bounded memory/tasks/file descriptors. It binds IPv4 only;
TURN TCP/UDP clients use 3478 with UDP relays 52000-52199. TURN/TLS is not provided
in this experiment (443 remains nginx's). This is still a candidate, not a proven
systemd-confined deployment.

The outbound chain applies only to the dedicated relay UID. It permits replies
from the 3478 listener only for established connections, runtime-host ICE UDP
50000-50999, own-public-IP relay UDP 52000-52199, and relay-source UDP to public
peers. Other private/local/reserved destinations, IPv6, arbitrary own-public-IP
ports, and other outbound traffic are dropped. Other UIDs return unchanged.
This is necessary because allowing the private runtime IP in coturn's IP ACL
alone would also expose unrelated services on that host. The dedicated account
must never run other applications. No global firewall flush or input rule is
generated. Restarting the egress unit with a pre-existing table fails closed;
review/replacement must occur with TURN stopped.

First test the actual nft rules in a fresh network namespace:

```bash
cd ~/src/4-play
sudo python3 tools/test-private-turn-egress.py
```

This requires sudo for kernel namespace/firewall operations. The parent launches
`unshare --net`; the child refuses to operate in PID 1's host network namespace.
All addresses, interfaces and nft rules are fixture-only within that namespace.
It checks nft syntax, systemd unit syntax, allowed publisher/public peer traffic,
blocked LAN/loopback/IPv6/bad-port traffic, listener replies, and that another UID
is unaffected. It neither loads host firewall rules nor installs/starts any
service. If namespace creation is disallowed, stop and report that error; do not
run the commands directly in the host namespace. Kernel test results and actual
confined coturn-service tests are pending until this owner step succeeds.

Linux may report a forbidden local OUTPUT packet as immediate `sendto` EPERM or
EACCES, rather than silently dropping it. The worker reports only those send
errors with a distinct status, which is accepted only for a blocked test case
whose receiver got nothing. Bind failures, routing errors, other worker failures,
or denial/non-delivery of allowed traffic still fail the test.

## Root-only service staging (does not activate TURN)

After the isolated egress test passes, stage a persistent candidate bundle:

```bash
cd ~/src/4-play
sudo python3 deploy/systemd/stage-private-turn.py \
  --local-ip 192.168.20.68 \
  --public-ip 45.24.46.155 \
  --apply
```

Without `--apply`, this only checks the installed binary and describes the step.
The reference fingerprint is the default; another build requires its own tested
`--binary-sha256`. Apply revalidates the binary hash and root-owned/non-writable
ancestors, requires Debian coturn to remain masked and both candidate services to
be inactive, and reruns the kernel namespace/systemd syntax test before writing
anything persistent. It then creates `/etc/4play-turn` root-only mode 0700 with:

- `turnserver.conf`: fresh random shared secret and bounded relay configuration.
- `egress.nft`: UID-scoped candidate outbound policy.
- `4play-turn.service` and `4play-turn-egress.service`: **staged here only**, not
  installed into systemd's unit search path.
- `stage-receipt.json`: non-secret settings, artifact fingerprint and file hashes.

All files are root-owned mode 0600. Secrets are neither printed nor accepted in
arguments. An exact retry preserves the original secret and makes no changes;
different, symlinked, insecure, unexpected or incomplete files cause refusal,
never automatic replacement. An interrupted partial staging operation requires
operator review rather than silently generating a new credential.

This step does not install/enable/restart services, load host firewall rules,
alter application environment or nginx, rotate existing application tokens, or
enable public access. Runtime systemd-confinement and production-config relay
tests remain separate gates before installation/activation. Do not manually
start the staged unit or copy its credential contents into chat.

## Next operator action: actual confined TURN service validation

The reference host owner completed private staging and the actual nft namespace
checks successfully. Next run:

```bash
cd ~/src/4-play
sudo python3 tools/test-private-turn-service.py
```

This verifies root-only staging against the renderer and receipt, hashes the
installed binary, then creates a fresh network namespace (loopback/fixture IPs
only, no physical interface/default route). The kernel egress checks run there
again. A randomly named **transient** `4play-turn-validation-<uuid>.service`
starts the real patched coturn binary in that namespace, copying the reviewed
service's hardening/resource settings. Only the credential path, runtime directory,
logging and test lifecycle are overridden; `Restart=no`, `RuntimeMaxSec=90s` and
`Type=exec` prevent a runaway/restarting fixture. Permanent units are not installed
or started. Cleanup stops only that exact transient unit and releases the namespace.

The test uses a new throwaway shared key, never the production staged key. systemd
loads its fixture credential privately; neither key is printed or placed in
arguments. It confirms the real process UID, zero effective/permitted/inheritable/
bounding/ambient capabilities, no-new-privileges and separate network/mount
namespaces. Actual authenticated UDP and TCP TURN clients then relay a payload
to a fixture public address, verify invalid/expired credentials are denied,
confirm an unrelated private IP is rejected by coturn and the runtime's web port
is blocked by nft even though its IP is allowed, and check allocation ports are
released while coturn is still running. No packet reaches the real public network.
This fixture currently supports only the staged reference IP pair.

Portable command/process-validation tests and the existing local relay/media tests
can run without sudo. Actual transient-service results remain pending until the
owner runs the command above. This is **protocol and confinement** validation, not
yet relayed browser audio/video/input under confinement, genuine NAT traversal or
WAN responsiveness acceptance. Those gates remain required. Runtime/credential
directories and a root-private fixture log are temporary; no staged secret,
application service, nginx config, host firewall or router setting is changed.

The first owner run passed confinement, UDP/TCP relay, private-service denial and
invalid-credential rejection, but the namespace-wide socket-empty assertion timed
out. That measurement was incorrect: `/proc/PID/net/udp` lists sockets for the
whole network namespace, not that process alone. The revised check joins the
table to coturn's file-descriptor socket inodes, observes each advertised live
allocation before use, requires a zero-lifetime refresh acknowledgement and
waits for those exact allocation sockets to disappear after each release. It
does not ignore an observed allocation that remains open, extend its timeout,
or treat unreadable ownership information as success. A genuine remaining
allocation now reports only its port(s), never a credential. The corrected
privileged run is still pending; the earlier timeout is not claimed as a pass.

The namespace/credential behavior follows the official
[systemd execution documentation](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html)
and [transient service documentation](https://www.freedesktop.org/software/systemd/man/latest/systemd-run.html).

## Confirmed native policy-denial bug and separately tested local candidate

The corrected privileged test confirmed an actual retained allocation socket,
not just unrelated namespace sockets. This was reproduced autonomously in a new
unprivileged user/network namespace: normal UDP/TCP traffic releases correctly;
adding a send to the firewall-blocked runtime web port yields EPERM, after which
the zero-lifetime Refresh is acknowledged but its allocation socket remains.
Disabling optional recvmmsg did not fix it. The pinned native code marks the
relay socket to-be-closed after this send error, then excludes to-be-closed
sockets from Refresh family selection, skipping the deletion timer. This retains
the original lifetime rather than proving a permanent leak. Earlier measurement
fixes remain useful, but did not resolve this native error path.

Source evidence: pinned upstream
[UDP send handling](https://github.com/coturn/coturn/blob/4.18.0/src/apps/relay/ns_ioalib_engine_impl.c)
and [Refresh family selection](https://github.com/coturn/coturn/blob/4.18.0/src/server/ns_turn_server.c).

`deploy/systemd/coturn-relay-policy-errors.patch` is a **local downstream candidate,
not a new upstream release**. It treats Linux EPERM/EACCES on plain UDP relay
sends as a dropped datagram, consistent with existing recoverable UDP-drop paths,
without marking the whole allocation to-be-closed. It changes no firewall/ACL,
authentication or cryptography. The candidate must remain distinct and maintained
explicitly until an upstream resolution replaces it; production promotion is not
automatic. No upstream issue/PR has been filed on the owner's behalf.

The patch SHA-256 (LF-normalized) is:
`3e9fb96a00625369f612a79f9dcc79010df86583279c9fca0459c6f901c7fc5f`.
The builder verifies the upstream commit and patch before compiling in a fresh
private directory. It records separate candidate provenance and never installs.

Reference private artifact:
`/tmp/4play-coturn-4.18.0-4play-policy1-nqj93n0n/source/bin/turnserver`

Artifact SHA-256:
`3038dfe8786facafeb5be2138f77df6489b89f8af4953ff60150cadc9c41bc15`

Both unprivileged reproduction cases pass with this candidate, including blocked
web-port delivery, continued legitimate relay traffic after that denial, invalid
credentials and observed allocation-socket release. Existing real forced-relay
decoded audio/video, SCTP input, admission revocation/expiry, LAN isolation and
async-cleanup checks also pass with the candidate. Those media checks are still
loopback fixtures, not full systemd-constrained media or actual internet tests.
The user-namespace reproduction adapts only the fixture UID and client socket
mark to model unfiltered non-TURN callers. Never copy this adaptation into the
production nft table. EACCES follows the same native branch; the actual firewall
reproduction exercises EPERM, not both possible errno values.

Next copy the exact tested candidate into its own protected path, then test it
under the actual transient systemd restrictions:

```bash
cd ~/src/4-play
sudo python3 deploy/systemd/install-coturn-policy-candidate.py \
  --build-directory /tmp/4play-coturn-4.18.0-4play-policy1-nqj93n0n \
  --sha256 3038dfe8786facafeb5be2138f77df6489b89f8af4953ff60150cadc9c41bc15 \
  --apply
sudo python3 tools/test-private-turn-service.py --policy-candidate
```

The first command validates pinned candidate receipt/hash, never runs the input
binary as root, and installs only root-owned binary/provenance under
`/opt/4play-coturn/4.18.0-4play-policy1`. It refuses conflicting files and leaves
the upstream binary and `/etc/4play-turn` unchanged. The second checks the
root-owned candidate receipt/permissions again and selects it only for the
isolated transient validation unit; all sandbox properties and existing staged
files are still validated. No production service or host firewall is activated.
The privileged candidate run was reported PASS on the reference host, including
UDP/TCP authentication, denied private-service access, invalid credentials,
observed allocation release and actual systemd confinement. Do not rerun the default baseline and
expect a different result; it intentionally still tests the unchanged upstream
binary. Both the native fix and the final deployment need review before enabling
public gameplay. No additional router or UFW rule is needed for these tests.

Non-root reproduction/rebuild (temporary artifacts are retained):

```bash
python3 tools/reproduce-turn-release.py --turnserver /ABSOLUTE/CANDIDATE/PATH
python3 deploy/systemd/build-coturn-policy-candidate.py --build
```

The installed baseline reproduces the failure and exits nonzero; do not label
that outcome a successful security test. A new build has a different fingerprint:
retest it and use its actual receipt/hash rather than copying the reference hash.

Still required before public activation: finish deployment tests,
real-browser credential refresh acceptance, secret rotation/private tester enrollment setup,
explicit forwarding/UFW changes and genuine remote browser acceptance tests.

### Private media gate: routing and confinement passed

`tools/smoke-turn-media-profile.py` runs without sudo in a disposable user/network
namespace against the installed policy candidate. It uses fresh fixture credentials,
the production candidate validator and production bandwidth/allocation limits.
It does **not** exercise nft UID isolation or systemd confinement. No physical
interface, router, live service, staged credential or host firewall is changed.

```bash
python3 tools/smoke-turn-media-profile.py
```

The fixture has one private dummy interface. Its simulated public relay address
is bound directly on loopback, with a fixture-only public/public external mapping:
there is no router/NAT in this namespace. This is a routing adaptation, not evidence
that WAN NAT/forwarding works. Native test receivers use bounded UDP-only ICE;
TCP TURN protocol coverage remains separate from decoded UDP media coverage.

Observed on 2026-10-19: initial relayed decoded audio/video and SCTP press/release
pass, and a simultaneous host-only LAN spectator continues. A clean player
leave/rejoin can pass with the UDP-only receiver. Adding the relayed spectator
then fails with coturn error 486, `Allocation Bandwidth Quota Reached`, followed
by the publisher rejecting an answer without a relay candidate. The earlier
receiver with speculative TCP allocations hit the limit during player reconnect.
That first run was not a successful media acceptance test. Do not raise/remove
production quotas to make this fixture pass.

Follow-up diagnosis on 2026-10-19: twelve alternating UDP/TCP allocation/release
cycles pass with a bandwidth budget for **only one allocation**, provided each
cycle waits for the observed coturn socket to close. The media fixture had only
waited for `Nice.Agent.close_async` completion: this is not the same as coturn's
deferred native allocation-deletion timer expiring. It now also observes the old
allocation inodes disappearing before rejoining. For a spectator it tracks only
that spectator's inodes, preserving the player's live allocations.

With that correction the production-profile routing smoke passes decoded AV,
SCTP press/release, player leave/rejoin, spectator leave/rejoin, admission
revocation/expiry, unaffected LAN viewing and final native allocation release.
All allocation/bandwidth quotas and production candidate validation are unchanged.
This establishes clean leave/rejoin, **not** unrestricted immediate overlapping
reconnect or browser multi-interface capacity. Rapid retry/backoff and capacity
headroom remain rollout considerations; a cleanup callback alone must not be
presented as proof that server-side capacity is already free.

Reproduce the tighter independent bandwidth-reuse check without sudo:

```bash
python3 tools/test-coturn-loopback.py \
  --turnserver /opt/4play-coturn/4.18.0-4play-policy1/turnserver --capacity-reuse
```

The operator reported the extended privileged gate **PASS** on 2026-10-19 at
commits `76468d8` and `de9d451` (the latter includes WAN replacement grace),
including decoded AV, input, clean leave/rejoin, revocation,
expiry, native allocation deletion and transient service/namespace cleanup:
`sudo python3 tools/test-private-turn-service.py --policy-candidate --media`.
It validates staging unchanged and starts only a throwaway confined unit with a
fresh credential and the same simulated-public routing adaptation. The routing
smoke passes. SSH `sudo -n` requires a password, so the operator runs this gate
in their own terminal. Repeat it after publisher lifecycle changes. The existing
no-`--media` protocol gate remains unchanged.
The public site must remain staged until this and the other rollout gates pass.

### Credential refresh and WAN replacement

Browser WAN configuration now includes `credential_renew_after_ms=2700000`.
The browser uses a monotonic 45-minute timer to refresh media before both sides'
one-hour delegated credentials expire. Each refresh fetches fresh authenticated
configuration and creates a fresh offer; publisher configuration also derives
fresh TURN credentials for the new peer. No persistent secret enters browser code.
The current player slot or spectator grant is retained; spectators never acquire
an input channel. LAN configuration has no refresh timer.

This is a controlled **whole-peer reconnect**, not seamless in-place ICE restart.
Expect a brief media/control interruption at refresh. Stop/leave cancels the timer
and late configuration responses cannot resurrect a departed game. A busy slot
promotion defers the scheduling decision; an actual refresh failure exposes the
existing explicit Reconnect action instead of creating an unbounded retry loop.
Returning from a throttled background tab checks for overdue refresh. This does
not guarantee uninterrupted playback through browser suspension or lost networks.

For a matching existing WAN peer, the publisher now validates the new admission
before retiring it, waits at most five seconds for confirmed libnice deallocation,
then provides 1.25 seconds of deletion grace before creating the replacement.
The pool lock is not held while waiting; unrelated viewers continue receiving
media. Unconfirmed cleanup fails closed. This grace reduces temporary pressure;
it is not a capacity reservation or proof that arbitrary multi-interface browser
loads fit the current quotas. No TURN limits have been increased.

Automated checks cover monotonic scheduling, cancel/leave, delayed configuration,
fresh configuration requests, player/spectator ownership, invalid admission and
native WAN replacement with decoded AV/input. The operator's `de9d451` confined
gate passed. Remaining acceptance: test real browser WAN refresh beyond 45
minutes and verify Safari autoplay/input recovery. Internet
NAT/forwarding, multi-device capacity and latency remain separate pending gates.

```bash
node --test tools/test-browser-media.cjs tools/test-browser-media-lifecycle.cjs
python3 tools/test-webrtc-reconnect.py
sudo python3 tools/test-private-turn-service.py --policy-candidate --media
```

### Separate tested-candidate deployment bundle

The original `/etc/4play-turn` staging intentionally still refers to the upstream
binary that reproduced the firewall-denial cleanup bug. Do not install those old
units as the final relay. The new preparer creates a **separate inactive bundle**
for the installed, locally patched candidate:

```bash
python3 deploy/systemd/stage-tested-turn-candidate.py --describe
sudo python3 deploy/systemd/stage-tested-turn-candidate.py \
  --apply --accept-local-patch
```

The acknowledgement is explicit because `4.18.0-4play-policy1` is a downstream
patch, not an upstream release or a claim of upstream approval. This command
verifies the original protected staging and candidate provenance, checks TURN is
inactive and Debian coturn remains masked, and reruns the entire isolated
confined-media gate. Only after PASS does it create/reuse a fresh durable secret
and write five root-only files under `/etc/4play-turn-policy1`:
`turnserver.conf`, `egress.nft`, the two inactive service unit files and
`deployment-receipt.json`. All non-path service hardening and network limits are
identical. The receipt records the candidate provenance, baseline receipt hash
and file hashes, but never the secret. The original staging/binary is retained.

No unit is installed in systemd's search path, no daemon reload/start/enable is
performed, and no host nft/UFW rules, app environment, nginx or router changes
occur. Repeated matching runs preserve the same new secret and files; conflicts
or incomplete bundles fail closed. The gate failing prevents durable credential
generation and staging. The operator reported root bundle creation PASS at
`27a6ae6`, including the repeated confined media gate. Unprivileged plan output
alone is not evidence of successful installation.

After this step, reviewed unit installation/activation, shared-secret delivery to
the applications, development-token rotation/tester enrollment, narrow public
routes/forwarding and real remote-browser acceptance still remain. Do not open
public routes merely because the deployment bundle exists.

### Install the verified units inactive

```bash
sudo python3 deploy/systemd/install-tested-turn-units.py --apply
```

This verifies the full candidate bundle against the current protected baseline,
installed candidate provenance and canonical templates; stored hashes alone do
not authorize modified unit/policy contents. It requires TURN units inactive,
private units disabled and Debian coturn masked, and checks the actual staged
unit syntax with `systemd-analyze verify`. It then installs only these fixed unit
definitions under `/etc/systemd/system` and runs `daemon-reload`:

- `4play-turn-egress.service`
- `4play-turn.service`

Both definitions are root-owned and contain no secret values. Existing matching
files are retained; conflicting files/symlinks/unsafe metadata are rejected before
the first write. On a failed install/reload, only files created by this invocation
are removed, with device/inode checks to avoid deleting replaced files; existing
matching units are retained. The installer never runs start, stop or enable.

The root installation step remains pending. No host nft table/UFW rule, app
credential/environment, nginx, router or public-access change occurs in this step.
Do not run `systemctl enable --now` manually: reviewed relay activation and
application secret delivery are separate following steps. The old baseline
staging remains available but must not replace these candidate unit definitions.

Portable installer regressions:

```bash
python3 tools/test-tested-turn-units.py
python3 tools/test-tested-turn-stage.py
```

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
python3 tools/test-webrtc-turn-cleanup.py
python3 tools/test-private-turn-bundle.py
python3 tools/test-private-turn-egress-results.py
python3 tools/test-private-turn-stage.py
python3 tools/test-private-turn-service-unit.py
python3 tools/test-turn-socket-observation.py
python3 tools/test-turn-capacity-reuse.py
python3 tools/test-coturn-policy-candidate.py
python3 tools/test-coturn-policy-install.py
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
