# Two-Session Persistence Isolation

## Result

**PASS — 39 assertions on 2026-10-04.**

The validated harness is commit `f10ab7c`. The result bundle on the reference
runtime host is:

```text
/tmp/4play-persistence-isolation-20261004-133241
```

The repeatable command is:

```bash
cd ~/src/4-play
bash tools/two-session-persistence-isolation.sh \
  /tmp/4play-persistence-isolation-20261004-133241
```

## Scenario

The harness launches two concurrent WWF WrestleMania sessions with separate
session IDs, streams, input ports, virtual controllers, MAME processes,
FFmpeg encoders, and working directories. Both runtimes enable MAME autosave.

It then performs these checks:

1. Waits for both sessions to produce video and accept input.
2. Sends different coin and action histories to the two controllers.
3. Stops each runtime normally and requires a clean exit.
4. Verifies that both sessions wrote nonempty MAME autosave and NVRAM files.
5. Requires the autosave contents to differ and the NVRAM files to be distinct
   filesystem objects.
6. Restarts both sessions under `strace` and confirms that each MAME process
   opens only its assigned autosave and NVRAM paths.
7. Stops restored session A while session B remains active and confirms that
   session B's persisted files do not change.
8. Stops session B, verifies that the persisted objects remain isolated, and
   confirms that transient FIFOs and UDP input listeners are removed.

All 39 assertions passed.

## Graceful persistence flush

The runtime creates a private MAME Lua control script in each session working
directory. On normal `SIGINT` or `SIGTERM`, it writes a session-private stop
request. MAME observes that request, exits through its normal shutdown path,
and flushes autosave and NVRAM. The runtime waits up to three seconds before
using forced termination as a fallback.

This behavior was separately probed with a disposable TMNT session: the runtime
exited successfully, no MAME or FFmpeg child remained, and MAME wrote a
nonempty `auto.sta` file.

## NVRAM interpretation

Ordinary coin and attack input did not change WWF WrestleMania's NVRAM bytes.
Identical content is therefore not treated as evidence that files are shared.
The test establishes NVRAM isolation through separate device/inode identities,
session-specific paths observed at `openat`, exclusion of the peer working
directory from each trace, and unchanged peer hashes during independent
shutdown. Autosave files additionally have different content hashes.

## Regression evidence

After this test passed, both earlier suites were rerun against the same deployed
runtime:

- two-session media/controller/process isolation: **49/49 PASS** at
  `/tmp/4play-two-session-20261004-133326`
- abnormal child-failure isolation: **71/71 PASS** at
  `/tmp/4play-failure-isolation-20261004-133346`

These results close the Phase 1B runtime-isolation gate on the reference host.
