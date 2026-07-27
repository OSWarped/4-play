# ADR-0003: Use One Linux Machine for the Initial Server and Runtime Host

- Status: Accepted
- Date: 2026-07-27

## Context

The first implementation must fit existing hardware. The available local Linux server already hosts other applications, and there is no spare dedicated Windows machine.

## Decision

Phase 1 will deploy the control plane and one runtime host agent to the existing Linux server, subject to resource measurement and isolation.

Components remain separate processes or modules with explicit interfaces so the runtime agent can move to another machine later.

## Guardrails

- emulator sessions run with explicit CPU, memory, process, and filesystem boundaries where practical
- the trivia and portfolio applications must not share writable runtime directories
- session workload and encoder utilization must be measured
- a future Windows runtime host remains supported by architecture, not required by Phase 1

## Consequences

This minimizes hardware cost and accelerates learning. It also means Phase 1 results must record host specifications and concurrent workload so capacity conclusions are not overgeneralized.
