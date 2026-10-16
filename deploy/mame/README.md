# Headless startup warning acknowledgment

Apply `headless-warning-ack.patch` to the custom MAME checkout that already has
4-Play's raw video/audio output options. This is not a patch for stock MAME.

```bash
cd ~/src/mame-4play
git apply --check ~/src/4-play/deploy/mame/headless-warning-ack.patch
git apply ~/src/4-play/deploy/mame/headless-warning-ack.patch
git apply --check ~/src/4-play/deploy/mame/undumped-startup-exceptions.patch
git apply ~/src/4-play/deploy/mame/undumped-startup-exceptions.patch
make -j2 USE_QTDEBUG=0
```

Check `git apply --reverse --check` against the **second** patch first to detect
a fully updated checkout. The second patch modifies the first patch's warning
condition, so checking the first patch alone is not a reliable fully-updated
test. Do not apply either twice. Preserve unrelated local checkout changes.

The session runtime sets `FOURPLAY_MAME_ACK_WARNINGS=1` in its MAME child only.
The patch acknowledges only the non-critical emulation-status startup screen,
and only when **both raw output paths are configured**. It logs the complete
warning. Severe warnings, ROM loader warnings, BAD_DUMP/NO_DUMP flags, missing
ROMs/media and normal interactive MAME launches are not bypassed, except for
the two exact NO_DUMP cases described below. It does not
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

## Approved undumped-chip exceptions

`undumped-startup-exceptions.patch` applies **after** the first patch. Existing
hosts with the first patch need only apply this second patch, rebuild MAME and
deploy the updated `session-runtime`; no service restart is needed.

The runtime supplies `FOURPLAY_MAME_ACK_UNDUMPED=<game>` only for `rthunder` and
`rampage`. It explicitly clears that environment value for every other game.
The MAME patch independently verifies that game identity and requires:

- Rolling Thunder: exactly `hd637a01x0p.e3`, NO_DUMP, 4096 bytes.
- Rampage: exactly `e36a31axnaxqd.u15.bin`, NO_DUMP, 1 byte.
- Exactly one known-bad/undumped load count and exactly one matching NO_DUMP
  declaration across the complete device tree, with no BAD_DUMP declarations.
- No ROM-loader warnings (including wrong checksum/length) or severe emulation
  warnings. MAME's normal fatal required-file errors are unchanged.
- The original raw-video/raw-audio and explicit warning-ack opt-in gates.

MAME accepts these installed sets as **best available**, not perfect dumps. The
exceptions only acknowledge the startup warning: they do not manufacture a
missing dump, improve emulation, or relax installation audits. Warning text is
still logged. A future changed MAME ROM definition fails closed until reviewed.

```bash
python3 tools/test-mame-undumped-exceptions.py \
  --mame ~/src/mame-4play/mame --ini /opt/4play/config/mame \
  --rom-root /opt/4play/library/roms
```

This checks both positive streams, missing/wrong/disabled approval, deliberately
corrupted and incomplete **private copies**, and an unapproved Contra opt-in.
Original installed ROMs and live sessions are not modified.

Deploy the rebuilt MAME and `session-runtime` binaries together. The host agent
launches them for new sessions; no service restart is required when paths are
unchanged. Already running games continue with their old process image.
