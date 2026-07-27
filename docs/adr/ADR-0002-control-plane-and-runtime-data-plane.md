# ADR-0002: Separate Control Plane from Real-Time Runtime Traffic

- Status: Proposed
- Date: 2026-07-27

## Context

4-Play needs centralized discovery and orchestration, but controller input and gameplay media are latency-sensitive. Routing all traffic through the central API server would simplify topology while adding bandwidth, latency, and a single performance bottleneck.

## Proposed decision

The control-plane server would own identity, catalog, session state, host selection, and authorization.

After allocation, a seat would connect directly to the assigned runtime host for controller input, video, and audio using short-lived session-scoped credentials.

The server and runtime host agent may run on the same physical Linux machine in the first deployment.

## Alternatives considered

- relay all real-time traffic through the central server
- run a full emulator locally at every seat
- use a single monolithic process with no logical boundary

## Expected consequences

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
- direct connections may complicate reconnect and preview reuse

## Validation required

This ADR may move to Accepted after the Phase 1A feasibility work demonstrates that:

1. one seat can send input and receive synchronized video and audio from a runtime host on the wired reference network
2. the selected transport meets provisional latency and jitter goals
3. reconnect behavior is understandable and recoverable
4. runtime-host endpoints can be secured with session-scoped authorization
5. the approach does not prevent an efficient shared preview strategy

If these conditions are not met, relaying, hybrid routing, or local execution alternatives shall be reconsidered.
