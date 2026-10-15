# Safari browser media and compact game view

## Report and scope

iPhone 16 Pro Max / Safari: audio played, video stayed black without a spinner,
and the settings toolbar occupied most of the screen. The exact iOS version was
not recorded. Device acceptance remains a manual check; desktop layout previews
and GStreamer peers are not substitutes for Safari testing.

## Changes

- The raw-session publisher selects VP8 and Opus RTP payload numbers from each
  browser offer instead of assuming video 96 / audio 111. It rejects unsupported
  or non-receivable media explicitly. Codec names are case-insensitive; SDP and
  ICE credentials are never logged by the selector.
- Reproduction: a peer offering video 100 / audio 109 connected but decoded no
  media with the old publisher. With the fix, the mixed-payload multi-peer test
  decoded audio/video and verified isolated input, reconnect, and promotion.
  This confirms a compatibility defect, not the specific iPhone's root cause.
- The browser attaches a populated MediaStream, refreshing the composite stream
  when a second track arrives. Audio-first track delivery is regression-tested.
- Play / resume provides an explicit playback gesture without creating another
  session, changing volume, or renegotiating the peer.
- Seat settings collapse after configuration. On narrow screens the playing
  view hides browsing/settings, uses compact controls and safe-area spacing,
  and restores browsing when the game is left.
- No encoder buffering, codec family, audio timing, or firewall changes.

Safari supports VP8 WebRTC; this change does not require WebCodecs.
See [WebKit's VP8 announcement](https://webkit.org/blog/8672/on-the-road-to-webrtc-1-0-including-vp8/)
and [iOS video playback policies](https://webkit.org/blog/6784/new-video-policies-for-ios/).

## Automated validation

```powershell
node --test tools/test-admin-metadata-provider.cjs tools/test-admin-asset-upload.cjs tools/test-client-carousel.cjs tools/test-browser-input.cjs tools/test-browser-media.cjs tools/test-browser-media-lifecycle.cjs tools/test-browser-spectator.cjs
python tools/test-webrtc-offer-codecs.py
cargo test -p control-plane-server
```

On Linux with the browser-media GStreamer dependencies installed:

```bash
python3 tools/test-webrtc-offer-codecs.py
G_DEBUG=fatal-criticals python3 tools/test-webrtc-multi-peer.py
```

The multi-peer test uses synthetic raw media and does not launch or stop MAME.

## Activate and test

After pulling and building the control plane on the server:

1. Stop existing test games through the UI. Existing publisher processes retain
   the old Python code; reconnecting to an old game is not sufficient.
2. Restart the control plane (embedded client assets changed):

   ```bash
   sudo systemctl restart 4play-control-plane.service
   ```

3. Reload `/client` on the iPhone, or close and reopen that tab. Start a new game.
4. Check that the compact controls leave room for the video. If playback does
   not start, tap Play / resume once. Verify moving video and audible sound,
   portrait/landscape layout, leave, and a second launch.
5. Recheck a Windows player and a spectator. The follow-up touch controller is
   covered in [mobile touch validation](MOBILE-TOUCH-CONTROLS.md).

If video remains black, report the iOS version and receiver diagnostics: decoded
frame count/rate, video surface dimensions, paused/playing state, and connection
state. Do not share full SDP, grants, or tokens. The publisher's safe
`Negotiated offered RTP payloads` log can confirm the selected codec IDs.
