# 4-Play TODO

Last updated: 2026-10-31

This is the working execution backlog for 4-Play. Use
[ROADMAP.md](ROADMAP.md) for milestone direction and [STATUS.md](STATUS.md) for
the detailed implementation record. Keep this file concise: tasks should be
removed, moved, or checked off as decisions are made.

## Current

### Client V5 mobile controls

- [ ] Validate the landscape layout on iOS Safari across representative phone
  sizes and safe-area/notch configurations.
- [ ] Validate the landscape layout on Android Chrome across representative
  phone sizes.
- [ ] Test the floating joystick during sustained play, rapid direction
  changes, diagonals, and fighting-game motions.
- [ ] Tune the joystick dead zone, maximum visual travel, direction thresholds,
  opacity, and default visual position from play-test feedback.
- [ ] Evaluate action-button, Coin, and Start placement and sizing across games
  with different button counts.
- [ ] Verify rotation, backgrounding, reconnect, fullscreen, and Leave Game
  always release held input and restore normal browser gestures.
- [ ] Confirm the compact gameplay toolbar remains usable without materially
  reducing the landscape video area.
- [ ] Decide whether routine control visuals should remain persistent, dim while
  idle, or fade after players learn the layout.

### Client V5 rollout

- [ ] Keep Client V4 available as the stable fallback during V5 testing.
- [ ] Record V5 acceptance results and remaining usability issues.
- [ ] Decide whether V5 should replace V4 as the preferred client.
- [ ] If accepted, add V5 to the protected WAN route and its security/edge
  acceptance tests before publishing it remotely.

### Phase 3 closeout

- [ ] Complete the physical-table observations in
  [PHASE-3-PHYSICAL-CHECKLIST.md](testing/PHASE-3-PHYSICAL-CHECKLIST.md).
- [ ] Run a longer family/party soak and record control ergonomics, audio
  isolation, recovery behavior, and service access observations.
- [ ] Complete the Phase 3 closeout record using
  [PHASE-3-CLOSEOUT-TEMPLATE.md](testing/PHASE-3-CLOSEOUT-TEMPLATE.md).

## Next

### Desktop browser controllers

- [ ] Add visible Gamepad API connection and disconnection status to the browser
  player.
- [ ] Select the first controller that receives input and allow the player to
  choose another connected controller.
- [ ] Add a controller test view with live direction and button indicators.
- [ ] Verify the existing standard mapping with Xbox, PlayStation,
  Switch-style, and representative generic USB/Bluetooth controllers.
- [ ] Release all held input when a controller disconnects or the page loses
  focus.
- [ ] Evaluate per-controller dead-zone adjustment.
- [ ] Design optional button remapping and persistent controller profiles after
  testing the default mapping.

### Client usability

- [ ] Evaluate whether touch-control placement should be user-adjustable and
  saved per device.
- [ ] Add concise, non-blocking feedback for media loss, input loss, reconnect,
  and recovery without restoring permanent diagnostic text during gameplay.
- [ ] Review keyboard, touch, and gamepad discoverability together so each input
  method has a clear activation path.

### Admin and operations

- [ ] Perform a manual acceptance pass of the redesigned Admin console on
  desktop and mobile browsers.
- [ ] Continue the detailed Admin/Producer backlog in
  [ADMIN-PRODUCER-UI.md](design/ADMIN-PRODUCER-UI.md#outstanding-adminproducer-backlog).
- [ ] Refresh [STATUS.md](STATUS.md) so it reflects the browser clients, WAN
  enrollment, Admin redesign, and current Phase 3 state.

## Later

- [ ] Stabilize the emulator-adapter interface and add additional runtime hosts
  and emulators as described in Roadmap Phase 4.
- [ ] Define save, storage, package-validation, and package-import policies.
- [ ] Add fleet health, role-based administration, rollout/rollback, telemetry
  retention, and commercial hardening from Roadmap Phase 5.

## Deferred measurements

These measurements remain useful but do not currently block product work.

- [ ] Measure local and remote button-to-photon latency.
- [ ] Record median, 95th-percentile, and 99th-percentile latency.
- [ ] Measure rendered desktop-browser player CPU, GPU, and memory plateaus.

## Recently completed

- [x] Add Admin-managed user invitations with configurable and non-expiring
  lifetimes.
- [x] Redesign the Admin console around Overview, Sessions, Library, Access, and
  System workflows.
- [x] Suppress zoom, selection, callouts, dragging, and browser gestures during
  active mobile gameplay while preserving normal browsing behavior.
- [x] Add Client V5 as an isolated landscape-control experiment while retaining
  Client V4.
- [x] Add the Client V5 floating touch-origin joystick, overlaid action controls,
  compact toolbar, and larger landscape touch targets.
