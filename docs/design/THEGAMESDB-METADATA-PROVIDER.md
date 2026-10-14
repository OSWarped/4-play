# TheGamesDB metadata provider

## Scope

The optional control-plane provider supplies reviewed presentation metadata and
cached artwork. It does not launch games, scrape ROM archives, change MAME runtime
profiles, import player counts/controls, or automatically choose title matches.
The Admin/Producer UI remains separate from the seat client and emulator runtime.

The initial API adapter uses the documented title search with a platform filter,
game-ID lookup, genre/publisher dictionaries, and game-image listing. Platform
discovery requires one exact **Arcade** match; console results are filtered again
on the server. Source: [TheGamesDB official API specification](https://github.com/TheGamesDB/TheGamesDBv2/blob/master/API/spec.yaml).
Do not guess the arcade platform ID or assume a console port matches a MAME set.

## Review and publishing

1. Search a cleaned-up title; the visible ROM/catalog entry stays unchanged.
2. Select an Arcade candidate and review its descriptive fields and artwork.
3. Existing editor values default to unchecked. Empty descriptive fields may be
   preselected; all images start unchecked. The operator chooses replacements.
4. Optionally preview a chosen image through the authenticated server proxy.
5. Apply approved choices to the editor. Only approved fields are staged;
   downloaded images receive unique local filenames, never replacing existing files.
6. Save metadata through the existing editor to publish the changes. Browsing and
   cancellation do not publish metadata. If the editor changes during staging,
   the returned values are not applied over the new edits.

Allowed descriptive fields: sort title, description, genre, release year and
manufacturer. The manufacturer suggestion comes from the provider's publisher
dictionary; operator review matters when developer/manufacturer/publisher differ.
Strings are limited to the existing metadata field byte limits at UTF-8 boundaries.
Allowed image destinations: artwork, screenshot, marquee and logo. Boxart/fanart
default to artwork, banner to marquee, screenshot/titlescreen to screenshot, and
clearlogo to logo; the operator can choose a different destination.

Player-slot metadata, player count, control notes, game ID, ROM name, display name
and runtime profiles are outside the import whitelist. Existing uploaded Aliens
and Killer Instinct images remain selected until an explicit replacement is saved.

## Cache and provenance

Seven-day disk caches hold provider data and validated raster image bytes.
Paging URLs are discarded, reflected credential values are redacted, and cache
identities exclude the API key. Restarting the service retains fresh caches.
First-page searches return at most 25 candidates; refine a title rather than
blindly fetching every result. Review lists at most 50 artwork choices.

Imported images live under `media/<game>/thegamesdb/<provider-id>/` with UUID-based
filenames. Staging provenance is written under `<cache>/imports/`, including the
provider/game IDs, time, selected fields, original image filenames and local paths.
It records **staging**, not proof the operator subsequently saved metadata.
Successful staged assets are retained even if the editor is closed without saving;
automatic deletion of those files is intentionally not part of this increment.
In-process staging failure removes only files created by that request. No existing
artwork is removed. Provider metadata caches and downloaded content are gitignored.

## Security and resource policy

- `FOURPLAY_THEGAMESDB_API_KEY` is server-only. It is never returned to the UI,
  put in CDN requests, logged through raw HTTP errors, or committed to Git.
- Production API/CDN hosts are fixed HTTPS endpoints. Redirects are disabled.
  The browser cannot nominate a URL or turn the provider into an arbitrary proxy.
- Image filenames are validated as relative provider paths. Downloaded image
  signatures determine local JPEG/PNG/GIF/WebP extensions and MIME types. SVG/HTML
  provider downloads are rejected. Signature checks are not full image decoding.
- JSON is limited to 4 MiB; each image to 10 MiB, even without Content-Length.
  HTTP connect/request timeouts are five/twenty seconds.
- One acquisition request runs at a time; concurrent requests return 429 rather
  than building a queue. Uncached API calls are paced at least 250 ms apart.
  Provider 403/429 responses trigger a 60-second pause, or numeric Retry-After
  clamped to 60–3600 seconds. There are no automatic retries or background scans.
- Allowance display is the last live response's monthly snapshot, not a new quota
  lookup. Account limits remain provider/account-specific.
- Endpoints use the existing seat bearer authorization shared by Admin APIs.
  Separate operator roles/RBAC remain future deployment work; this is not a
  public, unauthenticated scraping service.

The integration does not establish redistribution/commercial rights to provider
content. Review provider terms and artwork permissions for the eventual barcade
deployment; an API key is not itself a content license.

## Next increments

After live validation: operator-requested cache refresh, paginated candidate
selection, persistent approved match mapping, published-import audit history,
selective batch jobs, and separate operator authorization. Do not enable unattended
bulk overwriting or scrape each time a seat browses the library.

See [configuration and validation](../testing/THEGAMESDB-METADATA.md).
