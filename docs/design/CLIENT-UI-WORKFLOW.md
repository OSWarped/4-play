# Client UI Workflow

This document describes how the 4-Play client front end should behave before
choosing a UI technology stack. The workflow should drive the implementation,
not the other way around.

## Product intent

The finished client should feel like an arcade/table front end, not a terminal
tool. Players should be able to start, join, rejoin, spectate, and leave games
without seeing ports, tokens, runtime hosts, or command-line prompts.

The current terminal `seat-input` client remains valuable as a development and
diagnostic tool, but it should not define the final player experience.

## UI roles

4-Play has three distinct UI roles:

1. **Player seat UI** — kiosk-style interface for starting, joining, rejoining,
   spectating, and playing.
2. **Spectator UI** — media-first mode for watching without reserving a player
   slot or sending input.
3. **Operator/admin UI** — management interface for sessions, catalog metadata,
   artwork, diagnostics, and system health.

Player and admin workflows do not need to live in the same application. The
seat UI should stay controller-friendly and safe for public use; the admin UI
can be form-heavy and operator-focused.

## Seat startup flow

```text
Seat app starts
  ↓
Load local seat identity
  ↓
Connect to control plane
  ↓
Fetch active sessions, catalog, seat state
  ↓
Show contextual home screen
```

The app should degrade gracefully:

- if the server is unreachable, show a reconnecting state;
- if the seat identity is missing, show setup/pairing guidance;
- if the catalog is empty, show an operator-facing hint;
- if a previous slot can be rejoined, make that the primary action.

## Contextual home screen

The home screen should choose the most useful primary action based on current
state.

Examples:

| Situation | Primary action |
| --- | --- |
| No active sessions | Browse games |
| Active game with open slots | Join active game |
| This seat has a disconnected slot | Rejoin player slot |
| No open slots but sessions active | Spectate |
| Runtime host unavailable | Wait / retry connection |

Secondary actions:

- Browse games
- Active sessions
- Spectate
- Controls/help
- Operator/admin entry, protected

## Game browser

The game browser should be artwork- and metadata-forward, similar in spirit to
arcade front ends such as Batocera or RetroPie, while preserving 4-Play's
session model.

Game card/list fields:

- logo or title treatment
- screenshot, preview, or placeholder art
- display name
- year
- manufacturer
- genre
- player count
- button/control summary
- availability
- running-session indicator

Game detail actions:

- Start game
- Join existing session
- Spectate existing session
- View controls
- View game details

## Active session browser

Active sessions should expose player slots clearly.

Example:

```text
Teenage Mutant Ninja Turtles
Running on Reference Linux

P1 Leonardo      occupied by Seat 1
P2 Michelangelo  open
P3 Donatello     open
P4 Raphael       open

Actions:
  Join as P2
  Join as P3
  Join as P4
  Spectate
```

If the current seat owns a disconnected slot, rejoin should be dominant:

```text
P2 Michelangelo disconnected from this seat

Primary action:
  Rejoin as P2
```

## Gameplay mode

During gameplay, the UI should get out of the way. The media view should be the
dominant surface.

The player should still have a controller-accessible overlay for:

- Resume
- View controls
- Leave game
- Rejoin, when applicable
- Spectate, when applicable
- Report problem / diagnostics shortcut
- Protected operator menu

The development `Esc` behavior is acceptable for terminal testing, but the real
client needs an input gesture available from the table controls.

## Spectator mode

Spectator mode is media-first and non-interactive with the game. It should be
visually distinct from a player slot.

Base spectator view:

```text
Spectating: Teenage Mutant Ninja Turtles
No controls are active
```

The action menu should be an overlay, not a permanent panel. When the spectator
moves the joystick or presses any button, the overlay should appear with a
slight slide animation:

- slide up from the bottom, or
- slide down from the top.

After a few seconds without input, the overlay should slide away again.

Suggested spectator overlay actions:

- Join open slot
- View controls
- Leave spectator mode
- Report problem

Rules:

- spectator input must not be forwarded as gameplay input;
- opening the overlay must not reserve a player slot;
- if a slot becomes available while spectating, joining should be explicit;
- the overlay animation should be subtle and quick enough not to distract from
  gameplay.

## Controls/help screen

Every game should have a controls/help surface derived from catalog metadata.

For a two-button beat 'em up:

```text
Move: Stick / D-pad
Button 1: Attack
Button 2: Jump
Attack + Jump: Special
Start: Start
Coin: Credit
```

For a six-button fighter:

```text
Punches:
  Quick / Medium / Fierce

Kicks:
  Quick / Medium / Fierce
```

This page should be available before launch, while joined, and while
spectating.

## Operator/admin workflow

Operator/admin should be separate from normal player flow.

Operator sections:

- Dashboard
- Active sessions
- Runtime hosts
- Seats
- Game library
- Metadata editor
- Artwork/media manager
- ROM validation/import
- Diagnostics and acceptance checks

Admin actions:

- stop stale sessions
- run smoke/acceptance checks
- inspect diagnostics artifacts
- hide/show games
- edit game metadata
- set preferred ROM variant
- manage artwork paths
- review missing assets

## Error and recovery UX

The UI should explain what happened and what the player can do next.

Examples:

| Error | Player-facing response |
| --- | --- |
| Control plane unavailable | Reconnecting to 4-Play server… |
| Runtime host offline | Games temporarily unavailable |
| Slot already occupied | Choose another open slot or spectate |
| Session launch failed | Game could not start; return to browser |
| Media receiver failed | Retry media, leave session, or report problem |
| Seat disconnected from slot | Rejoin player slot |

Error screens should include a short operator code or diagnostics reference, but
not raw stack traces.

## Derived technology requirements

The eventual UI technology should support:

- full-screen/kiosk operation;
- controller/keyboard navigation;
- animated overlays;
- local seat identity/configuration;
- HTTP API calls to the control plane;
- media playback or controlled launch of an external media receiver;
- low-latency input capture;
- clear packaging for Windows seats;
- future artwork-rich browsing;
- future admin/operator surfaces, either in the same app or a separate app.

The first implementation does not need to embed video playback if launching the
existing media path is faster and safer. The first goal is to replace terminal
selection prompts with a graphical workflow while preserving the proven runtime,
input, and media pipeline.

## Suggested UI milestones

### UI v0 — graphical shell

- full-screen home
- game list
- active session list
- start game
- join open slot
- rejoin disconnected slot
- spectate
- launch existing media/input path
- clean player-facing errors

### UI v1 — arcade browser

- artwork-backed game cards/details
- controls/help screen
- active slot map
- controller-first navigation
- spectator overlay animation

### UI v2 — kiosk polish

- embedded or tightly managed media playback
- attract/idle mode
- local settings/pairing
- startup integration
- polished transitions

### UI v3 — operator/admin

- dashboard
- metadata editor
- artwork manager
- ROM import/validation
- diagnostics browser
- acceptance artifact viewer
