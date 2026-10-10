# ADR-0005: Packaged native seat client for Version 1

## Status

Accepted

## Context

The browser-served `/client` prototype has proven the player-facing flow for
browsing games, starting sessions, joining/rejoining player slots, spectating,
and presenting safe runtime handoff information.

The remaining Version 1 gap is owning local seat responsibilities:

- storing seat configuration locally;
- calling the control-plane APIs;
- receiving private connection grants internally;
- launching and supervising the media receiver;
- capturing keyboard/gamepad input;
- forwarding authenticated input;
- never displaying or persisting session input tokens in the UI;
- exposing only safe runtime status back to the player interface.

We considered whether the browser should directly display the MAME stream and
send input. Browser input is viable, especially for a future mobile/touch
client. Browser media is not compatible with the current UDP MPEG-TS + ffplay
path without adding a browser-native transport such as WebRTC, MSE, or a custom
WebCodecs/WebTransport pipeline.

## Decision

Version 1 will use a packaged native Windows seat client.

The packaged client will use a web-style UI plus a local Rust backend. The Rust
backend owns native responsibilities and keeps private session grants out of the
browser-visible UI.

For Version 1:

- media may be launched as a supervised child process, initially ffplay;
- input forwarding should run in the native backend process memory, not as a
  child process with the session token in command-line arguments;
- browser-visible status and handoff data must be sanitized;
- session input tokens must not be rendered, copied, logged, persisted, or placed
  in process arguments;
- local config files may store seat identity, destination address, control-plane
  URL, and ffplay path, but must not store bearer tokens until an
  OS-protected credential store is selected;
- the existing control-plane/session/input APIs remain the source of truth.

The initial framework choice remains intentionally narrow: build framework-neutral
Rust backend contracts and process/input ownership first. A desktop shell such as
Tauri, Wry, WebView2, or another suitable wrapper can be selected once the native
backend boundary is stable.

## Consequences

This lets us solidify the working low-latency architecture quickly:

```text
control plane
  -> private session grant
  -> packaged native seat backend
  -> supervised media receiver
  -> in-process authenticated input forwarding
  -> safe status to web-style UI
```

It avoids prematurely rebuilding the media plane while preserving a clean path to
future browser-native/mobile clients.

## Future enhancement: WebRTC browser client

WebRTC remains on the roadmap for a future revision. A WebRTC transport could
allow no-install browser seats, mobile/tablet touch overlays, and browser-native
spectator playback:

```text
MAME/runtime media
  -> WebRTC media transport
  -> browser video element via MediaStream

browser keyboard/gamepad/touch
  -> WebRTC data channel or WebSocket
  -> authenticated input routing
```

That future work should reuse the same control-plane APIs, player-slot model,
catalog metadata, and public/private runtime handoff boundary.

## Implementation notes

As of 2026-10-10, `clients/seat-client-native` contains the first native backend
foundation:

- token-safe launch plans for player and spectator runtimes;
- a media process supervisor for ffplay-compatible receivers;
- in-process authenticated UDP input forwarding;
- an app facade for refresh, start, join, rejoin, spectate, join-from-spectator,
  leave, stop, status, and controller-state send operations;
- JSON view models for game cards, active session cards, joinable player slots,
  spectator counts, preview assets, and current runtime status;
- local non-secret config save/load. The API token is intentionally supplied at
  runtime rather than stored in the config file;
- a simple `seat-client-native` CLI for writing a token-safe config file and
  printing a token-safe snapshot view model.

Useful development commands:

```powershell
cargo run -p seat-client-native -- init-config `
  --config .\tmp\seat-native.json `
  --control-plane http://192.168.20.68:8080 `
  --seat-id windows-seat-1 `
  --destination-ip 192.168.20.10 `
  --ffplay-path .\path\to\ffplay.exe

$env:FOURPLAY_SEAT_API_TOKEN = "<provisioned-seat-token>"
cargo run -p seat-client-native -- snapshot --config .\tmp\seat-native.json
```

The snapshot command is a backend smoke test and frontend integration aid. It
does not launch media or input yet; the app facade and runtime supervisor provide
that lower-level capability for the packaged UI shell.
