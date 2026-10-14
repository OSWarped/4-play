# TheGamesDB setup and validation

## Server setup (manual secret entry)

The code can run with this provider disabled. After the updated control-plane
binary is deployed, edit the existing private environment file on Linux:

```bash
sudoedit /etc/4play/4play.env
```

Add these settings, replacing `YOUR_ACTUAL_API_KEY` inside the editor:

```text
FOURPLAY_THEGAMESDB_API_KEY=YOUR_ACTUAL_API_KEY
FOURPLAY_THEGAMESDB_CACHE=/home/blake/src/4-play/data/thegamesdb
```

Use your **TheGamesDB** API key here, not the 4-Play seat token. Never paste the
real key into chat, a shell command, a browser field, Git, or a diagnostic bundle.
Do not print the complete environment file or process environment to verify it.
An empty/missing key disables acquisition; malformed keys fail configuration
without displaying their value. The configured cache must be writable by the
control-plane service user (currently `blake`); it is created on first use.

Keep the environment file root-private, then activate between test games:

```bash
sudo chmod 600 /etc/4play/4play.env
sudo systemctl restart 4play-control-plane.service
```

No host-agent restart or inbound firewall rule is needed. The control plane needs
outbound HTTPS to the official API/CDN. Open `/admin`, Ctrl+F5, and save your usual
**4-Play seat API token** in the Admin token field. Never enter the provider key
there. Select a game, expand **Find metadata & artwork on TheGamesDB**, and confirm
the status says it is configured on the server.

## First live acceptance

1. Search **Aliens**, choose an Arcade candidate, and inspect its title/year.
2. Verify your existing description/artwork remains untouched and unchecked.
3. Preview one provider image; verify the browser sends the request only to the
   local authenticated `/api/v1/metadata/thegamesdb/image` route, not an API-key URL.
4. Select only one descriptive field you actually wish to change, or a new image
   for a chosen artwork field. Approve replacement of an existing editor value.
5. Apply choices to editor. The catalog is unchanged until **Save metadata**.
6. Save, refresh `/client`, and confirm the desired result in the carousel.
   Player counts, controls and ROM/runtime identity must stay unchanged.
7. Repeat the review for **Killer Instinct**, checking it is the arcade game, not
   a console port. Existing uploaded artwork is not automatically replaced.
8. Repeat a search/review to exercise caching. Test reopening the editor and
   entering a different game while a request is pending; late responses must
   not apply to the replacement editor.

No live TheGamesDB call has been validated with your key yet. Fixture results
prove our contracts/guards, not provider account access, completeness or image
availability. The exact platform discovery and real response shapes need this
first live acceptance before batch scraping is considered.

## Automated checks

```powershell
cargo test -p control-plane-server -- --test-threads=1
node --test tools/test-admin-metadata-provider.cjs tools/test-admin-asset-upload.cjs tools/test-client-carousel.cjs tools/test-browser-input.cjs tools/test-browser-media.cjs tools/test-browser-media-lifecycle.cjs tools/test-browser-spectator.cjs
cargo fmt -p control-plane-server -- --check
cargo clippy -p control-plane-server --all-targets -- -D warnings -A clippy::too_many_arguments
```

The Clippy exception is for existing service constructors/store methods with
large argument lists; no unrelated APIs were refactored for this integration.
Fixture tests run without a real key and never call TheGamesDB. They cover
authentication/disabled mode, arcade filtering, non-arcade rejection, dictionary
mapping, cache reuse after restart, credential-free caches/errors, bounded paths,
backoff, isolated staging, preservation of existing files/metadata, and the
browser review/staging workflow. HTTPS support is explicitly enabled for the
provider; the test upstream is an isolated loopback HTTP fixture.

## Limitations and recovery

- 403/429: verify the private key/account allowance; wait for the enforced pause.
  Do not hammer retries or copy a raw credential-bearing provider URL into logs.
- No match: shorten/refine the query; first-page results are deliberately bounded.
- Existing image stays visible: uncheck/keep it to preserve it, or explicitly
  select/import a replacement and then save metadata.
- Staging failure: the current catalog stays untouched. Correct cache/asset-root
  permissions or the reported provider problem, then retry.
- Closed editor after successful staging: local staged images and provenance
  remain, but no catalog value is published automatically.
- Longer provider descriptions are clipped to existing field limits before
  review; full source responses remain in the local provider cache.
