# Game Package Design

## Purpose

A game package describes how 4-Play presents and launches a title without exposing arbitrary runtime commands to clients.

A package references locally supplied runtime content; it does not embed copyrighted ROMs, BIOS files, or scraped media.

## Minimum manifest

```yaml
schema_version: 1
id: mame.xmen.4p
title: X-Men
runtime:
  adapter: mame
  machine: xmen
players:
  minimum: 1
  maximum: 4
  slot_mode: fixed-character
media:
  marquee: media/marquee.png
  preview: media/preview.mp4
controls:
  profile: arcade-standard
```

## Package responsibilities

- presentation metadata
- compatible runtime adapter
- approved launch identifiers
- player-slot semantics
- control profile
- aspect ratio and rotation
- save and configuration policy
- preview and artwork references
- health-check hints
- package version

## Editable catalog metadata

The Phase 2 control plane stores administrator-editable presentation metadata
beside runtime-discovered catalog entries. This is the foundation for a future
admin module and a Batocera/RetroPie-style seat browser.

Current editable fields:

- sort title
- description
- genre
- release year
- manufacturer
- player-count override
- artwork path
- marquee path
- screenshot path
- logo path
- control notes

The metadata API accepts relative asset paths only; path traversal, absolute
paths, drive-qualified paths, and control characters are rejected. A
player-count override updates the effective catalog profile and the number of
player slots allocated for newly launched sessions.

Runtime discovery and library presentation are intentionally separate. MAME
`-listxml` remains the authoritative arcade identity/technical source, while
scraped or hand-edited metadata is stored as a presentation layer that can later
apply to MAME, RetroArch, or another runtime adapter. The intended scraper
pipeline is:

1. identify installed content through the runtime adapter, starting with MAME
   XML for arcade ROMs;
2. enrich the local catalog from provider sources such as ScreenScraper,
   ArcadeDB, TheGamesDB, or IGDB where credentials/licensing permit;
3. download artwork, marquees, screenshots, logos, manuals, and preview videos
   into a local asset cache;
4. store only relative asset paths in metadata;
5. preserve manual overrides so administrators can fix mismatches or cabinet
   preferences without fighting the scraper.

The control plane can serve local cached assets from a configured asset root.
Set `FOURPLAY_ASSET_ROOT` on the control-plane process and fetch assets through
`GET /api/v1/assets/<relative-path>`. If no asset root is configured, asset
serving remains disabled.

Current endpoints:

```text
GET /api/v1/games/{game_id}/metadata
PUT /api/v1/games/{game_id}/metadata
GET /api/v1/assets/<relative-path>
```

Development admin workflow:

```powershell
cargo run -p catalog-admin -- `
  --control-plane http://192.168.20.68:8080 `
  --api-token <admin-or-seat-api-token> `
  list

cargo run -p catalog-admin -- `
  --control-plane http://192.168.20.68:8080 `
  --api-token <admin-or-seat-api-token> `
  report

cargo run -p catalog-admin -- `
  --control-plane http://192.168.20.68:8080 `
  --api-token <admin-or-seat-api-token> `
  show tmnt

cargo run -p catalog-admin -- `
  --control-plane http://192.168.20.68:8080 `
  --api-token <admin-or-seat-api-token> `
  set tmnt `
  --player-count 4 `
  --genre "Beat 'em up" `
  --marquee-path media/tmnt/marquee.png

cargo run -p catalog-admin -- `
  --control-plane http://192.168.20.68:8080 `
  --api-token <admin-or-seat-api-token> `
  set-slot tmnt 2 `
  --label Donatello `
  --position P2 `
  --character Donatello `
  --artwork-path media/tmnt/p2.svg

cargo run -p catalog-admin -- `
  --control-plane http://192.168.20.68:8080 `
  --api-token <admin-or-seat-api-token> `
  export --output catalog-metadata.json

cargo run -p catalog-admin -- `
  --control-plane http://192.168.20.68:8080 `
  --api-token <admin-or-seat-api-token> `
  import --input catalog-metadata.json

cargo run -p catalog-admin -- `
  --control-plane http://192.168.20.68:8080 `
  --api-token <admin-or-seat-api-token> `
  validate-assets

cargo run -p catalog-admin -- `
  --control-plane http://192.168.20.68:8080 `
  --api-token <admin-or-seat-api-token> `
  seed-placeholders `
  --asset-root /opt/4play/assets `
  --update-metadata

cargo run -p catalog-admin -- `
  --control-plane http://192.168.20.68:8080 `
  --api-token <admin-or-seat-api-token> `
  seed-known-metadata `
  --asset-paths
```

This CLI is an interim admin module. `report` summarizes how complete each
game's browser presentation metadata is before a future graphical admin UI
exists. Exported metadata files use a stable `games[]` JSON shape so
administrators can back up metadata, bulk edit it, and eventually seed a
graphical admin interface that uses the same metadata endpoints. `set-slot`
edits per-game player slot labels, positions, character hints, and slot artwork
paths. `validate-assets` checks every artwork, marquee, screenshot, logo, and
slot artwork path against the configured local asset endpoint.
`seed-placeholders` creates copyright-safe SVG placeholders under the asset root
using the conventional `media/<game-id>/` layout, and can optionally update
metadata to point at those files. `seed-known-metadata` fills conservative
presentation defaults for the current reference catalog while preserving manual
edits unless `--overwrite` is passed; `--asset-paths` also links the conventional
placeholder/asset paths. Scrapers can later replace those placeholder files with
provider-sourced media without changing the catalog schema.

## Validation

A validator shall reject:

- unknown schema versions
- path traversal
- absolute executable paths
- arbitrary command fragments
- missing required assets
- unsupported adapters
- duplicate package IDs
- invalid slot/controller mappings

Runtime adapters construct command lines from typed package fields and administrator configuration.
