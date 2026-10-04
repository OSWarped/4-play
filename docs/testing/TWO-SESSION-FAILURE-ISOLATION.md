# Two-Session Failure Isolation

## Result

**PASS — 71 assertions on 2026-10-04.**

The validated implementation is commit `196d7ca`. The result bundle on the
reference runtime host is:

```text
/tmp/4play-failure-isolation-196d7ca
```

The repeatable command is:

```bash
cd ~/src/4-play
bash tools/two-session-failure-isolation.sh 5 \
  /tmp/4play-failure-isolation-196d7ca
```

## Scenarios

The harness launches TMNT as session A and Aliens as session B with separate
streams, input ports, virtual controllers, MAME processes, FFmpeg encoders,
and working directories.

It then performs these tests in order:

1. Proves both sessions produce decodable media and session B accepts input.
2. Sends `SIGKILL` to session A's MAME child.
3. Requires session A to exit nonzero and clean up its remaining process,
   controller, input port, and media FIFOs.
4. Proves session B's video advances from 156 to 511 frames and its controller
   still accepts input.
5. Restarts session A with the same session ID and ports and proves media and
   input work again.
6. Sends `SIGKILL` to session A's FFmpeg child.
7. Repeats the cleanup checks and proves session B advances from 857 to 1,152
   frames while still accepting input.
8. Restarts session A a second time and proves media and input work again.
9. Stops both sessions normally and verifies final cleanup.

All 71 assertions passed. A final residue check found no session runtime,
MAME, FFmpeg, or failure-test UDP listener left behind.

## Runtime changes validated

- The runtime polls both MAME and FFmpeg while it is active.
- An unexpected exit from either child terminates the session with a nonzero
  result after attempting every cleanup stage.
- Killing FFmpeg causes the runtime to terminate MAME rather than leaving an
  orphan emulator running.
- Killing MAME causes the runtime to reap the dependent encoder.
- Virtual controllers and input listeners are removed on failure.
- `video.raw` and `audio.pcm` FIFOs are transient and removed when the runtime
  exits.
- `cfg` and `nvram` directories remain persistent across failure and restart.

## Child-exit ordering

When MAME is killed, its FIFO closure can let FFmpeg report a clean EOF before
the parent observes MAME's wait status. The runtime treats either observation
as an abnormal child exit and returns failure. The harness records the child
it deliberately killed but does not assume a kernel-defined reap order for
dependent children.

## Remaining Phase 1B work

Concurrent save/NVRAM isolation remains unvalidated. The runtime now preserves
the required per-session directories, but a test must still write distinct
game state in two live sessions, restart both, and prove each session restores
only its own state.
