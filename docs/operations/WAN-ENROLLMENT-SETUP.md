# Configure protected WAN enrollment without exposing the site

Reference host: `blake@192.168.20.68`. This step prepares application enrollment
and rotates application API tokens. It does **not** activate the public nginx
candidate, restart applications, remove the local-only TURN guard, or change
UFW/router rules. Existing trivia and other nginx sites are untouched.

## Prerequisites

- The operator's root TURN credential installation passed on 2026-10-21.
- Applications were restarted and LAN `/client-v4` still plays correctly.
- Both application services run as `blake` with the known base environment and
  `40-4play-wan-media.conf` drop-ins. Unknown extra drop-ins/configuration fail
  preflight instead of being overwritten.
- The existing private invitation database is
  `/home/blake/.local/share/4play/tester-access/access.sqlite3`, owned by `blake`,
  private and readable by the invitation CLI. Existing invitations are retained.
- HTTPS still returns 503 at the arcade hostname; the dedicated TURN guard and
  egress tables are present. This remains a local-only deployment checkpoint.

## 1. Update and test (as blake)

```bash
cd ~/src/4-play
git status --short
git pull --ff-only origin main
cargo build --release -p control-plane-server --bins
python3 tools/test-wan-enrollment-setup.py
python3 deploy/systemd/configure-wan-enrollment.py --describe
```

If `git status` shows unexpected local changes, stop before pulling and preserve
them. No privileged work is performed by the tests or `--describe`.

## 2. Preflight, then install (sudo)

```bash
sudo python3 deploy/systemd/configure-wan-enrollment.py
sudo python3 deploy/systemd/configure-wan-enrollment.py --apply
```

Run the second command only if preflight passes. Installation prints no keys.
It generates three independent random 64-hex values (seat API token, host API
token, and nginx edge key); successful reruns retain the same values. It refuses
partial/conflicting settings instead of silently rotating again. File creation
is exclusive; failed validation/reload rolls back only newly created files and
empty directories using inode checks. The original `/etc/4play/4play.env` is
retained unchanged for rollback.

Installed files:

| File | Purpose |
| --- | --- |
| `/etc/4play-wan-enrollment/service-tokens.env` | New seat/host API tokens; root-only |
| `/etc/4play-wan-enrollment/control-plane.env` | Exact HTTPS origin, private edge key and database path; root-only |
| `/etc/4play-wan-enrollment/runtime-host.env` | Same database path; root-only |
| `/etc/nginx/snippets/4play-wan-edge-key.conf` | Matching edge key; root-only and not activated by this tool |
| `45-4play-wan-enrollment.conf` in each application's `.service.d` directory | Loads overrides after the existing base and media environment files; no key literals |

The origin is exactly `https://arcade.blakemilam.com` (no trailing slash). It is
not added to LAN enrollment origins; no public networks are added to LAN CIDRs.
The persistent TURN key is neither changed nor copied.

## 3. Activate applications only, while games are idle

First close all games and spectator views. Existing sessions are not migrated
across API-token rotation. Then run each command in order:

```bash
sudo systemctl stop 4play-runtime-host-agent.service
sudo systemctl restart 4play-control-plane.service
sudo systemctl start 4play-runtime-host-agent.service
systemctl is-active 4play-control-plane.service 4play-runtime-host-agent.service 4play-turn.service
```

Expect three `active` lines. A service remaining active does not alone prove host
registration or playback: confirm the catalog loads, start a game at
`http://192.168.20.68:8080/client-v4`, and test video, sound, sustained movement,
leaving the game, and (if available) another joining player or spectator. Reload
the browser after rotation; old cookies may need automatic reenrollment.

Confirm public staging is unchanged:

```bash
curl --silent --show-error --max-time 10 --output /dev/null --write-out '%{http_code}\n' \
  --resolve arcade.blakemilam.com:443:127.0.0.1 \
  https://arcade.blakemilam.com/client-v4
```

Expected: `503`. No nginx reload is needed in this step.

## 4. Update older token-based consumers privately

Browser `/client-v4` users do not enter service tokens. Server and local host
agent both receive the matching new API tokens from their shared override file.
The Admin UI's saved token and legacy `seat-input`/native-client environments do
not update automatically and will reject the old development token.

If you use those tools, view the new seat token **only in your own private terminal**:

```bash
sudo less /etc/4play-wan-enrollment/service-tokens.env
```

Use only `FOURPLAY_SEAT_API_TOKEN` for the Admin UI's API-token field and legacy
seat tools. Do not distribute the runtime-host token. Do not paste either value
into chat, Git, public links or screenshots. Token-based tools must be
reprovisioned through a private mechanism; the installer does not reach Windows
machines or update their local secret stores.

## Failure / rollback

If preflight or installation fails, stop; do not loosen file permissions or
remove existing guards. Do not paste private files, environment dumps or full
process arguments into chat. Output from successful commands above is token-safe,
except the deliberately private `less` view.

If application startup or LAN playback fails after activation, public access is
still staged. To restore the original application environment, disable only the
two new enrollment drop-ins by moving them to names not ending in `.conf`:

```bash
sudo mv /etc/systemd/system/4play-control-plane.service.d/45-4play-wan-enrollment.conf /etc/systemd/system/4play-control-plane.service.d/45-4play-wan-enrollment.conf.disabled
sudo mv /etc/systemd/system/4play-runtime-host-agent.service.d/45-4play-wan-enrollment.conf /etc/systemd/system/4play-runtime-host-agent.service.d/45-4play-wan-enrollment.conf.disabled
sudo systemctl daemon-reload
sudo systemctl stop 4play-runtime-host-agent.service
sudo systemctl restart 4play-control-plane.service
sudo systemctl start 4play-runtime-host-agent.service
```

Retain the private generated files and disabled drop-ins for review; do not
delete the invitation database, TURN configuration or media credential drop-ins.
This rollback temporarily restores the old development API tokens and disables
the WAN gate, so the public nginx site **must remain on staging**. A subsequent
setup attempt after rollback requires explicit review of the retained files.

## Next gate (not performed here)

Locally verify invitation-required enrollment, rejection of forged proxy headers,
and denial of public admin/raw API paths using the protected nginx candidate.
Only after those checks should public site activation and precisely scoped TURN
firewall/router changes proceed. Real remote direct/forced-relay gameplay and
long-running credential refresh remain acceptance tests, not established results.

### Automated local acceptance check

Operator checkpoint: enrollment configuration installation, application restarts,
staging 503 and LAN `/client-v4` gameplay have all passed. Run the next check on
Linux after updating the repository:

```bash
cd ~/src/4-play
python3 tools/test-local-wan-edge.py
sudo python3 tools/check-local-wan-edge.py --run
```

The first command uses synthetic keys, a temporary Rust server/database and
temporary nginx; it does not access production settings. The second requires
sudo to read the installed private snippet and real TLS key. It validates known
configuration files, starts its **own** nginx on two temporary `127.0.0.1` ports,
and routes requests through the reviewed candidate to the live application.
The real certificate is checked for `arcade.blakemilam.com`; validation is not
disabled. A connection bound to the server's LAN source address exercises actual
nginx client-IP replacement without allowing remote access to the fixture.

The live check creates exactly one one-hour, one-enrollment test invitation in
the real invitation database, redeems it in memory, and revokes it in cleanup.
Its revoked invitation/admission audit records are retained; existing tester
invitations are not revoked or consumed. A hard kill/power loss can interrupt
cleanup, so the temporary invitation is bounded to one hour. Do not run this
repeatedly as a monitor: the invitation store intentionally has a finite limit.
No invitation, cookie, service token or TURN credential is printed or saved in
a cookie jar. Requests never include secrets in URLs or process arguments.

Expected checks:

- Client/login assets load; admin and raw API paths return 404.
- Missing admissions cannot bootstrap or read the catalog/WAN ICE config.
- Forged direct/proxied edge/client-IP headers do not bypass authentication.
- A real invitation produces the required secure cookie and permits scoped
  catalog/WAN ICE reads, while unapproved origins/admin operations are rejected.
- The temporary invitation is revoked; its cookie immediately loses catalog access.
- The private nginx exits and public HTTPS still returns staging 503.

The checker never reloads the live nginx, starts/stops application services,
creates games, changes firewall/router rules or enables public access. On failure,
stop and report its token-safe output. External media and latency tests remain
separate even after this local check passes.

The operator confirmed every live local-edge check passed. Next:
[activate protected HTTPS while media remains closed](WAN-PROTECTED-EDGE-ACTIVATION.md).
