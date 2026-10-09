# Catalog Curation

The runtime host can verify and publish any MAME set listed in the catalog
manifest, including parents, clones, revisions, regional variants, and bootlegs.
The player-facing 4-Play catalog should stay curated rather than exposing every
verified archive.

## Selection rule

When multiple working MAME sets represent the same game, prefer one visible
catalog entry using this order:

1. United States release
2. World release
3. newest/most complete English-language release
4. parent/common set
5. other regional release
6. bootleg, prototype, or imperfect preservation only when no better working
   set is available or the variant is intentionally desired

The ROM directory may still keep additional sets because MAME split sets,
parents, clones, devices, and future testing may need them. The catalog is the
user-facing selection list, not a mirror of every archive on disk.

## Current curated choices

| Title group | Visible set | Reason |
| --- | --- | --- |
| Captain America and The Avengers | `captavenu` | US Rev 1.9 preferred over Asia Rev 1.4 and older US Rev 1.6 |
| Double Dragon II | `ddragon2u` | US preferred over World |
| Final Fight | `ffightuc` | USA set preferred over World |
| Golden Axe | `goldnaxeud` | only currently verified US Golden Axe variant |
| The Simpsons | `simpsons` | World set 1 preferred over later alternate World set |

If a cleaner parent or non-bootleg US/World set is later verified, replace the
catalog entry rather than adding a second visible duplicate.

## Quarantine rule

`mame -verifyroms` is necessary but not sufficient for the 4-Play catalog. A
game also needs to launch through the headless raw-media runtime and produce
video/audio frames without manual MAME UI acknowledgement.

If a set verifies but `session-runtime` reaches:

```text
session runtime did not receive video and audio
```

then remove it from the visible catalog until a runtime compatibility fix or a
better variant is available.

Sets under retest after enabling `skip_warnings` in the reference MAME UI
configuration:

| Set | Reason |
| --- | --- |
| `avengrgs` | verifies, previously produced no frames before warning bypass |
| `captavenu` | verifies, previously failed before media |
| `ddragon2u` | verifies/best available with redump warning, previously produced no frames |
| `ga2u` | verifies, previously produced no frames before warning bypass |
| `ssriders` | verifies, previously produced no frames before warning bypass |
