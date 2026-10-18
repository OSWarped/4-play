# Private tester invitation foundation

This is an operator-only foundation for WAN access, **not an enabled public
login flow**. The public hostname must continue returning the staging 503 page.
The follow-on [browser gate and experimental ICE implementation](WAN-BROWSER-ACCESS.md)
is now available behind explicit settings; it is not activated on the reference
host. LAN enrollment is unchanged.

## Implemented

- Dedicated SQLite storage, separate from the existing control-plane database.
- Random invitation codes and independent admission credentials, each with 244
  random bits. Only domain-separated SHA-256 hashes are persisted.
- Invitation expiration of at most seven days; admission validity at most eight
  hours and never past its invitation expiration. Reading/validating a credential
  never renews it or consumes another enrollment.
- One to eight lifetime browser enrollments per invitation; eight active
  admissions globally in this beta policy. Clearing cookies does not refund an
  enrollment. Capacity checks and redemption run in immediate transactions, also
  across separate database connections.
- Revocation persisted in the database; every admission authorization consults
  the invitation. Revocation rejects subsequent authorization even after restart.
- Up to 1,000 invitation records, each permitting at most eight admissions, so
  storage cannot grow without bound. No automatic deletion of revocation history.
  If the limit is reached, stop and review retention rather than deleting a live DB.
- Generic rejection errors for malformed, missing, expired, revoked, and spent
  invitation codes. No credential values in error messages or status listings.
- Private Unix files (0600), symlink/non-file rejection, and refusal of
  group/world-writable parent directories. The CLI refuses non-Unix operation;
  Windows unit tests use temporary files only, not production credential storage.

Revocation does **not** currently close existing game sessions, input channels,
or media grants. The HTTP gate is not wired yet. Its integration must check access
on requests and lease renewal and define a bounded disconnect/revocation policy.
Admission limits are not a substitute for public session, spectator, resource,
and request-rate limits.

## Automated checks

```bash
cargo test -p control-plane-server
cargo clippy -p control-plane-server --all-targets -- -D warnings -A clippy::too_many_arguments
cargo build --release -p control-plane-server --bins
```

Tests cover persistence, credential separation, code/admission expiry, revocation,
invalid settings, lifetime counts, global capacity, concurrent redemption on
independent connections, private Unix permissions, symlink rejection, storage
bounds, and strict operator CLI parsing. No live service or game is required.
The Clippy command allows the existing unrelated argument-count warnings; no
new Clippy warnings are suppressed in the invitation implementation.

## Operator CLI (for later activation)

Do not issue tester codes yet. Once the protected HTTP gate and WAN media path
are ready, run as the existing non-root 4-Play service user on Linux:

```bash
install -d -m 0700 "$HOME/.local/share/4play/tester-access"
./target/release/4play-tester-access issue \
  --database "$HOME/.local/share/4play/tester-access/access.sqlite3" \
  --hours 24 --seats 1
```

Only `issue` prints the invitation code, once, on the operator's terminal. Share
privately when testing is enabled; do not log, screenshot, or put it in a URL.
No redeem/authenticate command is exposed by the operator CLI.

Safe status and revocation commands:

```bash
./target/release/4play-tester-access list \
  --database "$HOME/.local/share/4play/tester-access/access.sqlite3"

./target/release/4play-tester-access revoke \
  --database "$HOME/.local/share/4play/tester-access/access.sqlite3" \
  --id INVITATION_UUID_FROM_ISSUE_OR_LIST
```

The database parent must already exist. `list` and `revoke` will not create a
database for a mistyped path. Private database files must not be committed to Git;
the documented path is outside the repository. Backups need the same privacy.

## Required before public activation

1. Explicit trusted proxy identity/address policy; reject direct header spoofing.
2. HTTPS same-origin enrollment form, HttpOnly/Secure cookie, CSRF protection,
   code-redemption rate limits, and scoped authorization on every request.
3. Bound public game/spectator capacity independently of the LAN; enforce it
   transactionally in the session lifecycle, not only in a browser button.
4. Private management endpoints; nginx public route allowlist; rotate development
   tokens. No shared service secret or admission credential exposed to UI scripts.
5. Internet ICE/STUN/TURN support, short-lived relay credentials, direct and
   forced-relay tests, then an external-device acceptance test.

See [rollout gates](../operations/WAN-TESTER-ROLLOUT.md).
