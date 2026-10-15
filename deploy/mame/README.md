# Headless startup warning acknowledgment

Apply `headless-warning-ack.patch` to the custom MAME checkout that already has
4-Play's raw video/audio output options. This is not a patch for stock MAME.

```bash
cd ~/src/mame-4play
git apply --check ~/src/4-play/deploy/mame/headless-warning-ack.patch
git apply ~/src/4-play/deploy/mame/headless-warning-ack.patch
make -j2 USE_QTDEBUG=0
```

If the patch is already applied, `git apply --reverse --check` succeeds instead.
Do not apply twice. Preserve unrelated local changes in this separate checkout.

The session runtime sets `FOURPLAY_MAME_ACK_WARNINGS=1` in its MAME child only.
The patch acknowledges only the non-critical emulation-status startup screen,
and only when **both raw output paths are configured**. It logs the complete
warning. Severe warnings, ROM loader warnings, BAD_DUMP/NO_DUMP flags, missing
ROMs/media and normal interactive MAME launches are not bypassed. It does not
inject any game input, alter ROMs, falsify emulation capabilities, or remove
Admin import audit warnings. Other environment values do not opt in.

`skip_gameinfo` does not skip emulation warnings. `skip_warnings` normally skips
only recently acknowledged matching warnings, so it does not solve fresh
session configuration directories. The raw stream contains emulated frames,
not this startup UI; a blocked warning produces zero raw video/audio and trips
the runtime startup watchdog.

Validate with the real installed ROMs, without touching live session state:

```bash
cd ~/src/4-play
python3 tools/test-mame-headless-warnings.py \
  --mame ~/src/mame-4play/mame --ini /opt/4play/config/mame \
  --knownbad-game contra
cargo test -p session-runtime
```

The probe uses fresh temporary state and FIFO drains, compares opt-out against
opt-in, and terminates only its own MAME processes. Never add `-str` or
`-seconds_to_run`: short runs themselves suppress startup screens and would
invalidate the test. A stream smoke test is not a gameplay-accuracy guarantee.

On 2026-10-15 the reference host passed opt-out/opt-in raw audio/video probes
for `elvact2u`, `indytemp` and `kidnikiu`, using the actual installed ROMs.
Invalid opt-in, missing raw-audio mode, and Contra's undumped-ROM warning all
remained blocked. The 10 Linux runtime unit tests passed. Runtime Clippy passed
with existing `too_many_arguments` and `let_and_return` lint allowances.

Deploy the rebuilt MAME and `session-runtime` binaries together. The host agent
launches them for new sessions; no service restart is required when paths are
unchanged. Already running games continue with their old process image.
