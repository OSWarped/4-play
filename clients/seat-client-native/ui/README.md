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

Security boundary:

- do not render session input tokens;
- do not persist bearer tokens in this UI;
- only consume `NativeSeatViewModel`, `NativeRuntimeStatus`, and
  `NativeControllerInput`-style safe payloads from the Rust backend.
