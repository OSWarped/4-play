# Contributing to 4-Play

## Current development posture

4-Play is architecture-first, but not architecture-only. Every phase should end with a demonstrable playable slice.

## Workflow

1. Start with or create a GitHub issue.
2. Keep changes focused on one vertical slice or one architectural decision.
3. Use a short-lived branch.
4. Add or update tests and documentation with the implementation.
5. Open a pull request describing behavior, tradeoffs, and validation.

## Branch naming

- `feature/<short-name>`
- `fix/<short-name>`
- `docs/<short-name>`
- `spike/<short-name>`

## Commit guidance

Use imperative commit messages that describe the outcome, such as `Add session reservation state machine` or `Prevent input routing across sessions`.

## Definition of done

A change is done when:

- behavior is tested at the appropriate level
- operational failures are observable
- configuration and startup steps are documented
- no secrets, ROMs, BIOS files, saves, or copyrighted media are committed
- architecture documentation remains accurate

## Architectural decisions

Create an ADR when a change materially affects deployment boundaries, network protocols, runtime ownership, persistence, media transport, security, or hardware assumptions.

Copy `docs/adr/ADR-TEMPLATE.md`, assign the next number, and mark the decision as Proposed or Accepted.
