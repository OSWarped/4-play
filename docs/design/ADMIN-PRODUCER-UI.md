# Admin/Producer UI

The Admin/Producer UI is a separate application from the seat client. It is the
operations console, bookkeeper, game-library manager, tournament desk, and
production booth for a 4-Play site.

## Goals

- Operate the site without SSH or terminal commands.
- Keep sessions, seats, runtime hosts, and catalog state understandable.
- Manage game metadata, artwork, visibility, and preferred ROM variants.
- Support tournaments and match queues.
- Open spectator feeds suitable for OBS or other streaming software.
- Record match results and operational notes.
- Run diagnostics, cleanup, smoke, and acceptance workflows.

## Non-goals for the first version

- Fully automated bracket management.
- Embedded OBS control.
- Automated VOD publishing.
- Multi-site fleet operations.
- Replacing the player seat UI.

These can come later once the admin/producer workflow is proven.

## Primary surfaces

```text
Dashboard
  ├─ Site health
  ├─ Active sessions
  ├─ Runtime hosts
  ├─ Seat status
  └─ Recent diagnostics / acceptance

Sessions
  ├─ Active sessions
  ├─ Player slots
  ├─ Spectator feeds
  ├─ Stop / cleanup
  └─ Failure history

Library
  ├─ Visible games
  ├─ Hidden/quarantined games
  ├─ Metadata editor
  ├─ Artwork/media manager
  └─ ROM validation/import

Events
  ├─ Tournaments
  ├─ Match queue
  ├─ Player/seat assignment
  ├─ Match result entry
  └─ Notes

Production
  ├─ Open spectator feed
  ├─ Follow active match
  ├─ OBS capture target
  ├─ Stream notes
  └─ Future overlays

Diagnostics
  ├─ Run cleanup
  ├─ Run strict smoke
  ├─ Run acceptance check
  ├─ View artifacts
  └─ Export support bundle
```

## Admin/Producer v0

The first checked-in Admin/Producer UI is intentionally small and dependency
free. The control plane serves embedded static assets at:

```text
/admin
```

The v0 console:

- accepts a bearer token in the browser and stores it in local storage;
- shows control-plane health;
- lists runtime hosts;
- lists active sessions and player-slot state;
- lists visible games and basic metadata;
- includes a production panel for selected sessions;
- shows a raw API snapshot for diagnostics.

The v0 console does not yet mutate state or create spectator grants. That is
intentional: the first milestone proves the admin/producer information
architecture before adding disruptive controls.

Next admin/producer increments:

1. create/release spectator grants from the production panel;
2. open a clean OBS capture window for a selected session;
3. add session stop/cleanup controls with confirmation;
4. add game metadata editing;
5. add event/match notes.

## Production spectator feed

The production spectator feed should be intentionally capture-friendly:

- clean window or browser surface;
- predictable aspect ratio and resolution;
- no accidental admin controls in the capture area;
- stable media endpoint;
- audio suitable for stream capture;
- clear session/match identity outside or around the captured area;
- optional future overlay region for player names, scores, round labels, or
  bracket context.

The first version may use the same underlying spectator media grants already
supported by the control plane. The important product behavior is that the
admin can choose a session and open a clean feed that OBS can capture.

## Tournament workflow

Initial tournament support can be lightweight and manual.

```text
Create event
  ↓
Choose game
  ↓
Create match queue
  ↓
Assign players to seats
  ↓
Launch/open match session
  ↓
Open production spectator feed
  ↓
Record winner and notes
  ↓
Advance queue
```

Minimum event fields:

- event name
- game
- date/time
- player names or handles
- match status
- assigned session
- result
- notes

## Admin safety

Admin actions can affect multiple players, so destructive or disruptive actions
should be explicit:

- stopping an active session should require confirmation;
- hiding a game should explain that players will no longer see it;
- ROM import/validation should not automatically publish broken games;
- cleanup should distinguish stale/non-terminal sessions from normal history;
- production spectator mode should not reserve player slots.

## Relationship to existing tools

The admin app can initially wrap existing APIs and scripts:

- control-plane game/session/runtime-host APIs;
- catalog-admin metadata and asset workflows;
- Phase 3 cleanup;
- Phase 3 diagnostics;
- Phase 3 smoke and acceptance checks;
- spectator grants for production feeds.

Over time, script-backed operations should become first-class API endpoints
with structured results.

## Technology implications

The admin/producer app likely has different technology requirements than the
seat client:

- rich tables/forms;
- authenticated operator access;
- file/artwork upload;
- long-running job status;
- artifact/log browsing;
- multiple monitor/window layouts;
- production feed windows suitable for OBS capture.

This reinforces keeping the admin/producer UI separate from the seat client UI.
