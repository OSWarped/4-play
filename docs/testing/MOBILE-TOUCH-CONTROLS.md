# Mobile browser touch controls

## Behavior

The browser player detects touch capability (`navigator.maxTouchPoints` or
`any-pointer: coarse`), rather than guessing from browser names. Players on
phones, tablets, and touch-enabled computers automatically get:

- A left-side eight-way directional pad, with a neutral center. Hold to keep
  moving; slide between directions without lifting.
- Numbered action buttons on the right, in runtime order. The session's
  `runtime_profile.buttons_per_player` determines the count, not the title or
  presentation player count. TMNT uses two; Killer Instinct uses six.
- Separate Coin and Start buttons. Multiple fingers can combine movement and
  attacks, including jump plus attack.

The six-action-button input protocol is unchanged. Counts outside 0–6 or missing
metadata produce an explicit message instead of guessed attack buttons. Join,
rejoin, reconnect, and spectator-to-player promotion carry the profile count.
Spectators never bind touch input or gain input authority from touching media.

Controls sit below the video, left and right, without covering gameplay. Narrow
screens use a compact layout; landscape phones shorten the video to leave room
for controls. Use the page's Fullscreen button to keep the controller with the
video when browser fullscreen is supported. Native video-only fullscreen may
hide HTML controls; exit it to use the touch controller. No orientation lock,
touchscreen vibration, custom per-game labels, or configurable layouts yet.

## Input safety and latency

Touch uses the existing authorized WebRTC input channel. Pointer changes send
immediately, and the existing 20 ms full-state heartbeat sustains holds; no
additional transport, queue, or timer is introduced. Keyboard, gamepad, and
touch states combine without one finger releasing another input source.

Pointer capture tracks each finger independently. Sliding outside the original
control group neutralizes that finger; it cannot turn an attack into Coin/Start.
Pointer-up, cancellation, lost capture, focus loss, hidden page, page exit,
rotation, disconnect, lease loss, and teardown clear held inputs. Reconnect
starts neutral. Focus changes between controls inside the player do not release
other fingers. Only the control panel disables scrolling/selection gestures;
the rest of the page retains normal browser interaction.

Implementation follows [Pointer Events](https://developer.mozilla.org/en-US/docs/Web/API/Pointer_events)
and [multi-touch guidance](https://developer.mozilla.org/en-US/docs/Web/API/Pointer_events/Multi-touch_interaction).

## Automated checks

```powershell
node --test tools/test-browser-touch.cjs tools/test-browser-input.cjs tools/test-browser-media-lifecycle.cjs
cargo test -p control-plane-server
```

Tests cover capability/count validation, held diagonal plus two attacks,
independent release/cancel/capture loss, sliding, shared keyboard/touch ownership,
Coin/Start, internal focus, backgrounding, rotation, teardown/reconnect, and
profile-count preservation through reconnect and spectator promotion.
Desktop narrow-window layout previews are not an iOS device acceptance test.

## Deploy and manually validate

Client assets are embedded in the control plane. After pulling and building:

```bash
sudo systemctl restart 4play-control-plane.service
```

Reload or close/reopen `/client` on the phone so the HTML, scripts, and CSS all
refresh. No firewall or runtime-host-agent change is needed for this update.

1. Start/join TMNT: verify two action buttons plus Coin/Start. Hold Right for
   several seconds; keep holding while tapping jump, then both attack buttons.
2. Slide Right to Down-right and back without lifting. Lift all fingers and
   confirm movement ends. Test Coin, release it, then Start.
3. Start/join Killer Instinct: verify six actions, combinations, and both rows.
4. Rotate portrait/landscape and background/restore Safari. Old held movement
   must not continue. Retouch to resume input after these safety releases.
5. Leave/rejoin and reconnect media. Verify controls return neutral with the
   correct count; the keyboard/gamepad path still works on Windows.
6. Spectate: no game controller should appear. Join an open slot through the
   spectator actions: touch controls should then appear for that game.

Record iOS version, any clipping/scrolling issues, and whether multi-finger
combinations register. A real iPhone retest is required before calling mobile
gameplay accepted.
