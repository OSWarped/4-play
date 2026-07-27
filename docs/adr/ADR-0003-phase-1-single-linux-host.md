# ADR-0003: Use One Linux Machine for the Initial Server and Runtime Host

- Status: Proposed
- Date: 2026-07-27

## Context

The first implementation must fit existing hardware. The available local Linux server already hosts other applications, and there is no spare dedicated Windows machine.

## Proposed decision

Use the existing Linux server as the default Phase 1 test host, contingent upon a hardware inventory and successful single-session emulation, capture, and encoding benchmark.

For Phase 1C, the control plane and one runtime host agent may share that machine if the earlier feasibility work shows adequate headroom and isolation.

Components shall remain separate processes or modules with explicit interfaces so the runtime agent can move to another machine later.

## Capacity gate

Before this ADR can be Accepted, document:

- CPU model, core count, and virtualization capabilities
- RAM capacity and normal utilization
- GPU model and available hardware encoders
- Linux distribution and display or headless-session constraints
- storage type, free capacity, and runtime-content location
- network-interface speed
- current trivia and portfolio workloads
- single-session emulator, capture, and encoding utilization
- expected Phase 1 concurrent-session count

## Guardrails

- emulator sessions run with explicit CPU, memory, process, and filesystem boundaries where practical
- the trivia and portfolio applications do not share writable runtime directories
- session workload and encoder utilization are measured
- tests record the host specification and concurrent background workload
- a future Windows runtime host remains supported by architecture, not required by Phase 1

## Validation required

This ADR may move to Accepted when:

1. one remote session operates without materially affecting the existing hosted applications
2. video and audio capture work reliably in the server's actual headless or desktop configuration
3. hardware or software encoding provides acceptable latency and resource usage
4. isolation and cleanup tests show that emulator failures do not disrupt unrelated workloads

## Expected consequences

This approach minimizes hardware cost and accelerates learning. It also prevents capacity conclusions from being generalized beyond the measured server configuration.
