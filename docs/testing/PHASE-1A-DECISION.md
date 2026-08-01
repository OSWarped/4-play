# Phase 1A Decision

## Status

Decision deferred. Media feasibility is promising, but remote input and measured latency are still required before accepting or rejecting the architecture.

## Evidence available

- headless MAME launch works
- Rust owns MAME and FFmpeg from one command
- synchronized live media reaches a remote Windows seat
- multiple games with different resolutions and refresh rates work
- CHD-backed content works
- two media sessions can run simultaneously
- Linux virtual-controller creation and generated events work

## Evidence still missing

- MAME response to the runtime-created controller
- seat-to-runtime controller delivery
- input timeout and disconnect neutralization
- local and remote button-to-photon distributions
- complete reference-host resource measurements
- full two-session controller and failure isolation

## Provisional interpretation

The direct seat-to-runtime architecture remains the leading approach. The current results justify continuing the experiment, but they do not justify beginning broad control-plane or Product MVP development yet.

## Next decision gate

Complete the following in order:

1. integrate the virtual controller into the session runtime
2. prove local runtime-generated input reaches MAME
3. implement a minimal remote controller-state sender
4. verify timeout and disconnect behavior
5. measure local and remote latency
6. update this document with a go, revise, or pivot decision

## Final decision

Pending.
