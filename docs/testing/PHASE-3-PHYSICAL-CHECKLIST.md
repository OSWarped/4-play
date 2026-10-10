# Phase 3 Physical Table Checklist

Use this checklist for the final physical-table observation pass. The software
acceptance checks are already automated; this file captures the parts that only
make sense at the real table with the actual controls, audio, screen layout,
and players.

## Session details

| Field | Value |
| --- | --- |
| Date | |
| Operator | |
| Table/cabinet location | |
| Linux host | `192.168.20.68` |
| Windows seat host(s) | |
| Acceptance artifact directory | |
| Soak artifact directory | |
| Games tested | |
| Approximate play duration | |

## Pre-flight

Run the automated acceptance gate before the physical pass:

```bash
cd ~/src/4-play
FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
tools/phase-3-acceptance-check.sh \
  --soak-results /tmp/4play-phase-3-soak-20261009-175455-manual
```

Record the generated `acceptance-summary.json` path above.

## Controls and ergonomics

| Check | Pass? | Notes |
| --- | --- | --- |
| Seat 1 controls are comfortable and reachable | | |
| Seat 2 controls are comfortable and reachable | | |
| Seat 3 controls are comfortable and reachable | | |
| Seat 4 controls are comfortable and reachable | | |
| Each player can see their character/status without crowding | | |
| Simultaneous movement/buttons work during active play | | |
| Leave/rejoin behavior is understandable from the seat | | |
| Spectator mode is understandable from a non-playing seat | | |

## Audio and display

| Check | Pass? | Notes |
| --- | --- | --- |
| Audio/video sync is acceptable during normal play | | |
| Audio level is comfortable for nearby players | | |
| Audio does not confuse which table/session is being played | | |
| Display is readable from all occupied seats | | |
| Media windows/receivers are positioned acceptably | | |

## Recovery behavior

| Check | Pass? | Notes |
| --- | --- | --- |
| One seat exits with `Esc`; other seats keep playing | | |
| The exited seat rejoins its previous slot | | |
| A spectator starts/stops without affecting players | | |
| Stopping a game returns all seats to a sane browser state | | |
| Final cleanup returns server to strict idle smoke pass | | |

## Closeout decision

Choose one:

- [ ] Phase 3 physical validation accepted.
- [ ] Phase 3 physical validation accepted with follow-up notes below.
- [ ] Phase 3 physical validation not accepted; blocking issues below.

Follow-up notes:

```text

```

Blocking issues:

```text

```
