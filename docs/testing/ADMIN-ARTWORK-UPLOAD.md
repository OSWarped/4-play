# Admin artwork uploads

## Workflow

1. Open `/admin`, save the API token, and select a game in the catalog editor.
2. Expand **Upload artwork/media asset** and choose Screenshot, Artwork, Marquee
   or Logo as the destination metadata field.
3. Choose a local file. An empty/automatically suggested destination is filled
   using the game ID, selected field, and file extension. Explicit custom paths
   are preserved. Example: `media/aliens/screenshot.jpg`.
4. Click **Upload asset**. Confirm replacement only when you intend to overwrite
   an existing file. The uploaded path is applied to the selected form field.
5. Click **Save metadata** to publish the reference. Refresh `/client` to see it.

JPEG, PNG, GIF, WebP and SVG images are supported. The server serves content types
based on destination extensions: renaming a JPEG to `.svg` does not convert it.
The Admin validates declared file type/filename against the destination before
uploading. This is a usability check, not a full image-content decoder or scanner.
The limit is 10 MiB; the server route's body limit now matches that advertised
limit instead of Axum's default 2 MiB. Other API routes retain their default limits.

Previews and the overwrite check use the actual protected `/api/v1/assets/...`
endpoint and bearer headers, not the nonexistent `/assets/...` alias. No token
is placed in asset URLs. Directly opening a protected URL in a new browser tab
still requires authorization; in-page previews fetch with the saved token.

## Reference artwork repair — 2026-10-14

The provided `Downloads/4-Play Images` folders contained a flyer, screenshot and
logo for both Aliens and Killer Instinct. Six original files were uploaded under
their original filenames in `media/aliens/` and `media/kinst/`, after confirming
those exact destinations did not exist. Catalog screenshot/artwork/logo/marquee
references were updated; descriptions, player counts and slot metadata were kept.
Both screenshots returned HTTP 200 and `image/jpeg` after the change. Existing
assets, including previously mislabeled files, were preserved and not deleted.

The reported failure included JPEG bytes stored in Aliens `screenshot.svg`, which
was returned as `image/svg+xml`; the catalog still referenced it. The old Admin
preview/overwrite endpoint also returned 404 even when the API asset existed.
The repair uses correctly named files, not content-type guessing.

## Tests and activation

```powershell
node --test tools/test-admin-asset-upload.cjs
cargo test -p control-plane-server -- --test-threads=1
```

Seven Node tests cover suggestions, extension mismatches, path safety, size limits,
actual uploader errors before writes, authenticated overwrite cancellation, and
the metadata-save reminder. Rust tests include uploads above 2 MiB and rejection
above 10 MiB without creating the oversized file.

Image/catalog changes are live immediately. The Admin fixes are embedded in the
control-plane binary: after deployment/build, restart the control-plane service
between games and hard-refresh `/admin`:

```bash
sudo systemctl restart 4play-control-plane.service
```

No runtime-host restart or firewall change is needed.
