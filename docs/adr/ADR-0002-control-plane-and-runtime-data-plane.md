# ADR-0002: Separate Control Plane from Real-Time Runtime Traffic

- Status: Accepted
- Date: 2026-07-27

## Context

4-Play needs centralized discovery and orchestration, but controller input and gameplay media are latency-sensitive. Routing all traffic through the central API server would simplify topology while adding bandwidth, latency, and a single performance bottleneck.

## Decision

The control-plane server will own identity, catalog, session state, host selection, and authorization.

After allocation, a seat will connect directly to the assigned runtime host for controller input, video, and audio using short-lived session-scoped credentials.

The server and runtime host agent may run on the same physical Linux machine in the first deployment.

## Alternatives considered

- relay all real-time traffic through the central server
- run a full emulator locally at every seat
- use a single monolithic process with no logical boundary

## Consequences

### Benefits

- preserves the terminal experience
- reduces unnecessary latency and bandwidth concentration
- allows runtime hosts to scale independently
- permits mixed Linux and Windows runtime hosts later

### Costs and risks

- more connection negotiation
- runtime hosts expose controlled network endpoints
- identity and authorization must be enforced consistently
- observability must correlate events across components
