# MAME Catalog and Runtime-Profile Validation

## Result

**PASS — 26 assertions on 2026-10-04.**

The validated implementation is commit `ead59c9`. The Linux result bundle is:

```text
/tmp/4play-runtime-host-smoke-20261004-174043
```

## Legal catalog boundary

`catalog/test-catalog.json` contains only stable game and MAME ROM identifiers.
It contains no ROM, CHD, BIOS, artwork, save data, or other copyrighted game
content. The initial allowlist is:

- Aliens (`aliens`)
- Killer Instinct (`kinst`)
- Teenage Mutant Ninja Turtles (`tmnt`)
- WWF WrestleMania (`wwfmania`)

The runtime agent invokes MAME's `-verifyroms` command using the managed MAME
configuration. A title is not published when verification fails.

## Automatic profile discovery

For every verified title, the agent reads `mame -listxml` and publishes:

- description
- native width and height
- native refresh rate
- display rotation
- maximum players
- maximum buttons per player
- save-state support

The control plane stores each profile against both its game and runtime host.
Catalog responses include the publishing host's current online/offline status,
so later allocation can require a compatible online host.

The live validation confirmed, among other fields:

| Game | Resolution | Players/buttons |
| --- | --- | --- |
| TMNT | 320×224 at 60 Hz | 4 players, 2 buttons |
| Killer Instinct | 320×240 | 2 players, 6 buttons |

## APIs

```text
PUT /api/v1/runtime-hosts/{host_id}/catalog
GET /api/v1/games
GET /api/v1/games/{game_id}
```

Catalog and runtime-profile records survive control-plane restart in SQLite.
The agent republishes its authoritative allowlist whenever it registers.

## Next step

Create durable sessions by selecting an online runtime profile, allocating
non-conflicting media/input ports, issuing a time-limited connection grant, and
recording canonical lifecycle transitions.
