# Activate invitation-protected public HTTPS (media still closed)

Prerequisites confirmed by the operator: private TURN commissioning, application
credential delivery, enrollment configuration/token rotation, application
restarts, LAN gameplay, and all checks in `check-local-wan-edge.py --run` passed.
That includes actual local nginx/Rust invitation redemption, secure cookies,
origin/header restrictions, catalog/WAN ICE reads, admin/raw API denial and
immediate rejection after revocation. This is **not** proof of internet media.

## Activate only the arcade website

On the Linux server:

```bash
cd ~/src/4-play
git pull --ff-only origin main
python3 tools/test-protected-edge-activation.py
sudo python3 deploy/nginx/activate-protected-edge.py --apply
```

The helper reruns the real local invitation gate before activation. That creates
and revokes one additional short-lived fixture invitation (retained for audit).
Do not run activation as a recurring monitor. No invitation or cookie is printed.

It accepts only the exact managed TLS staging or reviewed protected contents,
with the original root-owned enabled symlink. It atomically replaces only the
managed site's contents, checks `nginx -t`, reloads nginx, then verifies live HTTPS
asset access, required admission, and denied admin/raw API routes. Validation,
reload or HTTP postcheck failure restores/reloads the previous known contents.
Unknown/customized files or links are refused instead of overwritten.

The managed filename stays `/etc/nginx/sites-available/4play-arcade-staging` and
the existing enabled symlink stays in place. After activation, its **contents**
are the protected candidate, not the staging page. Keeping one enabled entry
avoids a duplicate hostname/gap while switching. Do not infer status from the
filename, and do not rerun `prepare-public-edge.py` against the protected site.

The helper does not edit keys, other virtual hosts, application services,
certificates, UFW/router rules or the local-only TURN guard. Existing trivia and
other sites retain their configurations. Check those sites normally afterward.

## Browser check

On a phone with **Wi-Fi disabled**, visit:

`https://arcade.blakemilam.com/client-v4`

An uninvited browser should be sent to the invitation form. It must not see the
game catalog until it redeems a valid invitation. `/admin` and `/api/v1/games`
must return 404 through this hostname.

For an owner-only login test, issue a fresh one-hour, one-browser invitation as
`blake` (no sudo). Use the code only in the browser form, never a URL or chat:

```bash
./target/release/4play-tester-access issue \
  --database "$HOME/.local/share/4play/tester-access/access.sqlite3" \
  --hours 1 --seats 1
```

Only this explicit `issue` command prints a secret. Entering it should allow the
catalog to load. Keep the test private and **do not start a game or invite other
testers yet**: external media is still blocked by design. This checkpoint is
website enrollment only. Public WAN signaling/media/firewall acceptance follows.

Report whether the invitation page appeared, login/catalog worked, private routes
were denied, and your existing sites/LAN still work. Never send the invitation.

## Immediate rollback

```bash
sudo python3 deploy/nginx/activate-protected-edge.py --apply --restore-staging
```

This restores the known 503 page using the same validation/reload transaction;
it does not require the application or invitation database to be healthy.
Keys, invitations, application configuration and media restrictions are retained.
If a rollback fails, stop and report the token-safe output; do not open media
ports or disable unrelated sites as a workaround.
