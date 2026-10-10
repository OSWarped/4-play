# Native seat UI validation

This checklist validates the `seat-client-native serve-ui` bridge on a Windows
seat machine.

## Prerequisites

- Linux control plane and runtime host are running.
- FFplay is installed on the Windows seat machine.
- The Windows repo is current with `origin/main`.

```powershell
cd C:\Users\blake\source\repos\4-play
git pull origin main
cargo build -p seat-client-native
```

Set the seat token in the current PowerShell session. Do not put this token in
the native seat config JSON:

```powershell
$env:FOURPLAY_SEAT_API_TOKEN = "<provisioned-seat-token>"
```

If needed, set the FFplay path:

```powershell
$env:FOURPLAY_FFPLAY_PATH = "C:\path\to\ffplay.exe"
```

The native launcher defaults to a 3x FFplay window scale. Override it only if
the window is too small or too large:

```powershell
$env:FOURPLAY_FFPLAY_SCALE = "4"
```

## Launch

```powershell
.\deploy\windows\Start-4Play-NativeSeat1.ps1
```

Open the printed URL, usually:

```text
http://127.0.0.1:40704
```

## Validate browsing

1. The game list loads.
2. Active sessions load.
3. Refresh shows a success message.
4. No API token or input token is displayed.

## Validate play

1. Click **Start** on a known-good game such as TMNT.
2. FFplay opens, receives media, and appears scaled larger than the original
   arcade resolution.
3. The UI status changes to player mode.
4. Keyboard input works:
   - `W/A/S/D` or arrow keys move.
   - holding a direction continuously moves until the key is released.
   - diagonal movement works.
   - `J/K/L/M/,/.` send action buttons.
   - simultaneous movement plus attack/jump works.
   - `1` inserts coin.
   - `2` starts.
5. If a gamepad is connected, validate:
   - left stick or D-pad moves.
   - buttons 0-5 send actions.
   - button 8 sends coin/select.
   - button 9 sends start.
6. Press `Esc` or click **Stop** and confirm:
   - the runtime stops or returns to browsing;
   - the local FFplay window closes instead of staying frozen on the last frame.

## Validate join/spectate

1. Start a session from seat 1.
2. Launch seat 2:

   ```powershell
   .\deploy\windows\Start-4Play-NativeSeat2.ps1
   ```

3. Open the printed URL, usually `http://127.0.0.1:40705`.
4. Confirm the existing session appears.
5. Join an open player slot.
6. Confirm seat 2 controls only its assigned player.
7. Spectate from a free seat and confirm it opens media without reserving a
   player slot.

## Expected failures to capture

If something fails, record:

- command output from the native seat PowerShell window;
- browser console errors;
- whether FFplay opened;
- whether the Linux session started;
- whether keyboard/gamepad commands show an error banner in the UI.
