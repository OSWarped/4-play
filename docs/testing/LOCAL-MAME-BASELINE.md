# Local MAME Baseline

## Status

Partially complete. Reliable headless launch and local runtime behavior are demonstrated. Objective local input latency measurements remain outstanding.

## Validated launch modes

### KMSDRM local-display validation

MAME was launched without X11 through SDL KMSDRM. A locally attached keyboard controlled the emulator after the launching user received the necessary Linux input-device permissions.

See `docs/experiments/MAME_KMSDRM_VALIDATION.md`.

### Headless raw-media validation

The current runtime launches MAME with SDL's offscreen video driver and custom raw-media outputs. The Rust session runtime now owns the MAME process.

Representative launch behavior:

```text
SDL_VIDEODRIVER=offscreen
MAME
  -inipath /opt/4play/config/mame
  -cfg_directory <session>/cfg
  -nvram_directory <session>/nvram
  -state_directory <session>/state
  -snapshot_directory <session>/snap
  -diff_directory <session>/diff
  -sound none
  -skip_gameinfo
  -rawvideowrite <session>/video.raw
  -rawaudiowrite <session>/audio.pcm
  <rom>
```

The normal development workflow now launches this configuration through `session-runtime` rather than by manually running the MAME command.

## Representative games

| Game | Resolution | Refresh rate | Result |
| --- | --- | --- | --- |
| Aliens | 288×224 | 59.185606 Hz | stable raw video and audio |
| TMNT | 320×224 | 60.000000 Hz | stable after correct refresh metadata |
| Killer Instinct | 320×240 | 58.981183 Hz | stable, including CHD-backed content |

## Current observations

- MAME maintained approximately real-time emulation speed during the validated media tests.
- Raw video frame boundaries remained complete.
- Raw PCM audio remained complete in 20 ms blocks when read by the Rust bridge.
- Per-game refresh rate must be preserved to avoid apparent A/V drift.
- Ctrl+C terminated the observed runtime process tree cleanly.

## Input baseline

A separate Rust tool creates a Linux virtual controller with two axes and eight buttons. Linux and `jstest` recognize and report the generated events.

MAME has not yet been proven to consume that virtual controller. Physical-controller and virtual-controller button-to-photon measurements have not yet been collected.

## Missing measurements

- local button-to-photon sample set
- median, 95th-percentile, and 99th-percentile local latency
- local display mode and refresh details for the latency baseline
- frame-pacing measurements beyond observed production rate
- CPU, GPU, RAM, disk, and power use
- repeatable physical input and camera methodology

## Completion criteria

This baseline is complete only after the same representative game, input device, and display configuration used for remote testing have a documented local latency distribution and resource profile.
