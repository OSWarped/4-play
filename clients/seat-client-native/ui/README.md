# 4-Play native seat UI scaffold

This folder is a dependency-light frontend scaffold for the packaged native seat
client. It is intentionally shell-agnostic: a future Tauri, Wry, WebView2, or
similar host can load these assets and bind one backend command bridge.

Expected backend bridge:

```js
window.__FOURPLAY_NATIVE__.command({ type: "refresh" })
```

The bridge should forward commands to `NativeSeatUiSession` in
`clients/seat-client-native`. If the page runs without a native bridge, it uses
mock data so layout and interaction work can continue without a live cabinet.

## Local bridge mode

The current development bridge is served by the `seat-client-native` binary:

```powershell
cargo run -p seat-client-native -- init-config `
  --config .\tmp\seat-native.json `
  --control-plane http://192.168.20.68:8080 `
  --seat-id windows-seat-1 `
  --destination-ip 192.168.20.10 `
  --ffplay-path .\path\to\ffplay.exe

$env:FOURPLAY_SEAT_API_TOKEN = "<provisioned-seat-token>"
cargo run -p seat-client-native -- serve-ui --config .\tmp\seat-native.json
```

Then open the printed local URL, usually:

```text
http://127.0.0.1:40704
```

`serve-ui` binds to localhost by default and exposes:

- `GET /` — seat UI
- `GET /app.js` and `GET /styles.css` — static assets
- `POST /native-command` — JSON `NativeUiCommand` bridge

This is not the final desktop package. It is an operational bridge that lets the
UI call the same Rust backend contract a packaged shell will use.

Security boundary:

- do not render session input tokens;
- do not persist bearer tokens in this UI;
- only consume `NativeSeatViewModel`, `NativeRuntimeStatus`, and
  `NativeControllerInput`-style safe payloads from the Rust backend.
