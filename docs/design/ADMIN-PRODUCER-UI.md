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
- accepts a producer/OBS destination IP and stores it in local storage;
- shows control-plane health;
- lists runtime hosts;
- lists active sessions and player-slot state;
- lists visible games and basic metadata;
- edits visible game metadata for library presentation and runtime player count;
- edits per-player slot metadata for future seat-selection and overlay use;
- uploads artwork/media assets under the configured asset root;
- includes a production panel for selected sessions;
- creates production spectator grants for selected sessions;
- releases production spectator grants when the producer is done with a feed;
- prints receiver URLs and ffplay commands for OBS/producer machines;
- opens a clean production capture helper page for OBS setup/overlay use;
- requests active session shutdown with a confirmation prompt;
- stores local producer notes for selected sessions;
- summarizes session diagnostics for active, stopping, failed, and stale sessions;
- requests stop for stale non-terminal sessions from diagnostics;
- shows a raw API snapshot for diagnostics.

The v0 console intentionally keeps disruptive controls out of scope. Production
spectator grants are safe because they do not reserve player slots.

Next admin/producer increments:

1. add artwork preview thumbnails and replacement warnings;
2. persist event/match notes server-side;
3. add deeper cleanup execution/results inside the console.

## Game metadata workflow

The Admin/Producer v0 console exposes a first-pass metadata editor from the
catalog grid. Selecting **Edit metadata** on a game opens a form for:

- sort title;
- genre;
- manufacturer;
- release year;
- player count;
- description;
- control notes;
- artwork, marquee, screenshot, and logo asset paths.
- optional per-player slot labels, positions, character names, and artwork paths.
- direct upload of artwork/media files to a relative asset path.

Saving uses the existing catalog metadata endpoint:

```text
PUT /api/v1/games/{game_id}/metadata
```

Asset uploads use the protected asset endpoint:

```text
PUT /api/v1/assets/{relative_asset_path}
```

The upload body is the raw file bytes. The path must be relative and cannot
contain traversal. After upload, the Admin/Producer UI applies the uploaded path
to the selected metadata field; the operator then saves metadata to keep the
reference.

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

The first version uses the same underlying spectator media grants already
supported by the control plane. OBS receives gameplay through the generated UDP
Media Source URL. The admin console also provides a clean `/admin/capture`
helper page that can be opened in a separate window or OBS Browser Source for
session identity, setup instructions, and future stream overlays.

### Validated OBS workflow

On 2026-10-09, the Admin/Producer v0 console successfully created a production
spectator grant for an active `tmnt2` session. OBS on the Windows producer
machine consumed the generated UDP MPEG-TS URL as a Media Source and displayed
the live gameplay feed.

Validated operator flow:

1. Open `/admin`.
2. Enter the bearer token.
3. Enter the producer/OBS machine IP.
4. Select an active session.
5. Click **Create production spectator feed**.
6. Copy the generated `udp://0.0.0.0:<port>?fifo_size=1000000&overrun_nonfatal=1`
   URL into an OBS Media Source with **Local File** unchecked.
7. Optionally open **Open clean production capture helper** for a clean
   producer/overlay surface tied to the generated media port.
8. When the production source is no longer needed, click
   **Release production spectator feed** in the Admin/Producer console.

This validates the first real Admin/Producer production use case: opening a
separate spectator feed suitable for live-stream capture without consuming a
player slot. The release action returns the dedicated media port to the
runtime host pool.

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

- stopping an active session requires confirmation and should explain that all
  connected players and spectators will be disconnected;
- hiding a game should explain that players will no longer see it;
- ROM import/validation should not automatically publish broken games;
- cleanup should distinguish stale/non-terminal sessions from normal history;
- production spectator mode should not reserve player slots.

## Session stop workflow

The Admin/Producer v0 console exposes a **Stop session** action in the
production panel for a selected active session. The action uses the existing
control-plane endpoint:

```text
POST /api/v1/sessions/{session_id}/stop
```

The browser confirmation is intentionally plain and explicit: stopping a
session ends gameplay for every connected player and spectator. The API moves
the session to `stopping`; the runtime host then shuts down the emulator and
releases runtime resources.

## Producer notes

The production panel includes local producer notes for a selected session. This
is intentionally browser-local in v0, using `localStorage`, so producers can
quickly record match callouts, winners, technical issues, or stream reminders
without introducing a server-side event model prematurely.

Future persisted notes should attach to an event/match/session record and
become part of the Admin/Producer bookkeeping workflow.

## Diagnostics summary

The Admin/Producer console summarizes session health above the raw API
snapshot. The first diagnostic view is read-only and derived from existing
session APIs:

- non-terminal sessions;
- sessions in `stopping`;
- failed/terminal sessions that need review;
- non-terminal sessions with no updates for more than ten minutes.

This gives an operator a quick triage view before reaching for cleanup tools.
For stale non-terminal sessions, the diagnostics list exposes **Request stop**,
which uses the same confirmed stop-session workflow as the production panel.
The next increment is to wire deeper cleanup execution and structured cleanup
results directly into the console.

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
