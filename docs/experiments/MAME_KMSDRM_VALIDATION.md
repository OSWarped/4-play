# MAME KMSDRM Runtime Validation

## Status

**Validated on July 28, 2026** on the initial Linux runtime host (`blake-web`).

## Objective

Prove that MAME can be launched without X11, receive input from a locally attached keyboard, and exit cleanly back to the supervising shell.

## Result

The experiment succeeded.

- MAME launched from an SSH session with no `DISPLAY` value.
- SDL selected `KMSDRM` as the active video driver.
- MAME rendered directly through the Intel GPU to the locally attached display.
- ALSA audio initialized successfully.
- The locally attached USB keyboard controlled MAME after the runtime user received permission to read Linux input devices.
- Pressing `Esc` ended MAME cleanly and returned control to the SSH shell.
- No X server, desktop environment, window manager, or graphical terminal was required.

## Validated launch command

```bash
mame -inipath /opt/4play/config/mame tmnt
```

The explicit `-inipath` argument is required because the 4-Play MAME configuration and ROM path are stored under:

```text
/opt/4play/config/mame
```

Launching bare `mame tmnt` used MAME's default configuration search path and therefore did not locate the configured ROM set.

## Video path

MAME reported:

```text
Available videodrivers: x11 wayland KMSDRM offscreen dummy evdev
Current Videodriver: KMSDRM
```

The validated video path is:

```text
MAME -> SDL -> KMSDRM -> DRM/KMS -> GPU -> local display
```

This proves that X11 is not required for the initial single-session runtime-host experiment.

## Keyboard root cause

MAME initialized a logical system keyboard, but initially received no key events. The physical SteelSeries keyboard appeared as Linux event devices owned by `root:input`:

```text
crw-rw---- root input /dev/input/event4
crw-rw---- root input /dev/input/event5
```

The `blake` user was not initially a member of the `input` group and could not read either device. After adding the user to that group and starting a new login context, both devices became readable and MAME received keyboard input.

Development fix:

```bash
sudo usermod -aG input blake
```

Verification:

```bash
id
test -r /dev/input/event4 && echo readable
test -r /dev/input/event5 && echo readable
```

## Clean exit

Pressing MAME's default `Esc` control terminated the emulator normally. The launching SSH shell remained available and regained control immediately.

This validates the minimum lifecycle needed by a future runtime supervisor:

```text
runtime launches MAME
        -> MAME owns video and input
        -> player/operator requests exit
        -> MAME terminates
        -> runtime captures exit status and performs cleanup
```

## Architectural implications

1. X11 is not a requirement for the validated single-session local runtime path.
2. The earlier keyboard failure was a Linux device-permission problem, not an SDL or MAME dependency on X.
3. The production runtime should use a dedicated service account rather than the developer account.
4. That account will require controlled access to video, rendering, audio, and intended input devices.
5. Membership in the broad `input` group is acceptable for development validation but may be overly permissive for production. A later hardening phase should evaluate systemd-logind seat ownership or targeted udev rules.
6. This result does not yet prove that several simultaneous MAME sessions can share one GPU or KMS device. Multi-session isolation remains a separate Phase 1B experiment.

## Follow-up work

- Create a supervised runtime launcher that always supplies the 4-Play INI path.
- Record MAME PID, start time, exit time, and exit status.
- Handle `SIGTERM` and abnormal process termination.
- Restore display and audio state after failures.
- Replace the development user with a dedicated runtime service account.
- Test USB arcade encoders and virtual input devices.
- Determine how KMSDRM interacts with concurrent runtime sessions and remote frame capture.
