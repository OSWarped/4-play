#!/usr/bin/env python3
"""
Experimental WebRTC publisher for a single 4-Play MAME session's raw FIFOs.

This is the first bridge from real MAME output to browser-native media:

    session-runtime video.raw/audio.pcm FIFOs
      -> GStreamer raw parsers/encoders
      -> webrtcbin
      -> control-plane /api/v1/sessions/{id}/webrtc/offer proxy
      -> browser <video>

It is intentionally standalone and opt-in. It does not replace the proven
UDP/FFplay path and should be started only for one manual validation session.
"""

from __future__ import annotations

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
import signal
import sys
import threading
import traceback
from pathlib import Path
from typing import NoReturn

import gi

gi.require_version("Gst", "1.0")
gi.require_version("GstSdp", "1.0")
gi.require_version("GstWebRTC", "1.0")

from gi.repository import GLib, Gst, GstSdp, GstWebRTC  # noqa: E402


PIPELINE_TEMPLATE = """
webrtcbin name=webrtc bundle-policy=max-bundle latency=0
filesrc name=video_source location={video_path} do-timestamp=true
  ! rawvideoparse name=video_parse format=bgrx width={width} height={height} framerate={fps_num}/{fps_den}
  ! queue name=video_queue leaky=downstream max-size-buffers=2 max-size-time=0 max-size-bytes=0
  ! videoconvert
  ! vp8enc deadline=1 cpu-used=8 target-bitrate={video_bitrate_bps} keyframe-max-dist={keyframe_distance}
  ! rtpvp8pay pt=96
  ! application/x-rtp,media=video,encoding-name=VP8,payload=96,clock-rate=90000
  ! webrtc.
filesrc name=audio_source location={audio_path} do-timestamp=true
  ! rawaudioparse name=audio_parse use-sink-caps=false format=pcm pcm-format=s16le sample-rate=48000 num-channels=2
  ! queue name=audio_queue leaky=downstream max-size-buffers=4 max-size-time=0 max-size-bytes=0
  ! audioconvert
  ! audioresample
  ! opusenc audio-type=restricted-lowdelay bitrate={audio_bitrate_bps}
  ! rtpopuspay pt=111
  ! application/x-rtp,media=audio,encoding-name=OPUS,payload=111,clock-rate=48000
  ! webrtc.
"""


class RawSessionPublisher:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.loop = GLib.MainLoop()
        self.started = False
        self.media_started = False
        self.pending_answer_context: dict[str, object] | None = None
        self.ice_timeout_id: int | None = None

        self.pipeline = Gst.parse_launch(
            PIPELINE_TEMPLATE.format(
                video_path=quote_path(args.video_path),
                audio_path=quote_path(args.audio_path),
                width=args.width,
                height=args.height,
                fps_num=args.fps_num,
                fps_den=args.fps_den,
                video_bitrate_bps=args.video_bitrate_kbps * 1000,
                keyframe_distance=args.keyframe_distance,
                audio_bitrate_bps=args.audio_bitrate_bps,
            )
        )
        self.webrtc = self.pipeline.get_by_name("webrtc")
        if self.webrtc is None:
            raise RuntimeError("pipeline did not create webrtcbin named 'webrtc'")

        self.webrtc.connect("notify::ice-gathering-state", self.on_ice_gathering_state)
        self.webrtc.connect("notify::ice-connection-state", self.on_webrtc_state_changed)
        self.webrtc.connect("notify::connection-state", self.on_webrtc_state_changed)
        self.webrtc.connect("notify::signaling-state", self.on_webrtc_state_changed)
        self.webrtc.connect("on-ice-candidate", self.on_ice_candidate)

        bus = self.pipeline.get_bus()
        bus.add_signal_watch()
        bus.connect("message", self.on_bus_message)

    def run_http_signaling(self) -> None:
        host, port_text = self.args.http_signal.rsplit(":", 1)
        server = ThreadingHTTPServer((host, int(port_text)), request_handler(self))
        thread = threading.Thread(
            target=server.serve_forever,
            name="4play-webrtc-raw-session-http",
            daemon=True,
        )
        thread.start()
        install_signal_handlers(self.loop)
        print(
            f"Raw session WebRTC publisher listening at http://{host}:{port_text}/sessions/{self.args.session_id}/offer",
            file=sys.stderr,
        )
        print("Leave this running while the MAME session is active. Press Ctrl-C to stop.", file=sys.stderr)
        try:
            self.loop.run()
        finally:
            server.shutdown()
            self.pipeline.set_state(Gst.State.NULL)

    def start_pipeline(self) -> None:
        if self.started:
            return
        for label, path in [("video", self.args.video_path), ("audio", self.args.audio_path)]:
            if not Path(path).exists():
                raise RuntimeError(f"{label} FIFO does not exist: {path}")
        print(
            "Prepared raw-session WebRTC pipeline "
            f"session={self.args.session_id} "
            f"video={self.args.video_path} audio={self.args.audio_path} "
            f"size={self.args.width}x{self.args.height} "
            f"fps={self.args.fps_num}/{self.args.fps_den}",
            file=sys.stderr,
            flush=True,
        )
        result = self.pipeline.set_state(Gst.State.READY)
        print(
            f"Raw-session WebRTC pipeline moved to READY result={result.value_nick}",
            file=sys.stderr,
            flush=True,
        )
        self.started = True

    def start_media_flow(self) -> None:
        if self.media_started:
            return
        print(
            f"Starting raw-session WebRTC media flow session={self.args.session_id}",
            file=sys.stderr,
            flush=True,
        )
        self.pipeline.set_state(Gst.State.PLAYING)
        self.media_started = True

    def answer_for_offer(
        self,
        offer_text: str,
        browser_host_ip: str | None,
        timeout_seconds: float = 8.0,
    ) -> str:
        if browser_host_ip:
            offer_text = replace_mdns_candidates(offer_text, browser_host_ip)
        event = threading.Event()
        context: dict[str, object] = {"event": event}
        GLib.idle_add(self.apply_offer_and_create_answer, offer_text, context)
        if not event.wait(timeout_seconds):
            state = self.pipeline.get_state(0)
            raise RuntimeError(
                "timed out waiting for GStreamer SDP answer; "
                f"pipeline_state={state.state.value_nick} pending={state.pending.value_nick}"
            )
        if "error" in context:
            raise RuntimeError(str(context["error"]))
        answer = context.get("answer")
        if not isinstance(answer, str):
            raise RuntimeError("GStreamer did not produce an SDP answer")
        if self.args.answer_file:
            Path(self.args.answer_file).write_text(answer, encoding="utf-8")
        return answer

    def apply_offer_and_create_answer(
        self,
        offer_text: str,
        context: dict[str, object] | None = None,
    ) -> bool:
        try:
            self.start_pipeline()
            result, sdp_message = GstSdp.SDPMessage.new_from_text(offer_text)
            if result != GstSdp.SDPResult.OK:
                raise RuntimeError(f"failed to parse browser SDP offer: {result}")
            offer = GstWebRTC.WebRTCSessionDescription.new(
                GstWebRTC.WebRTCSDPType.OFFER,
                sdp_message,
            )
            promise = Gst.Promise.new_with_change_func(self.on_remote_description_set, context)
            self.webrtc.emit("set-remote-description", offer, promise)
        except Exception as error:  # noqa: BLE001 - diagnostic publisher reports setup failures.
            print(
                f"Failed while applying browser SDP offer: {error}",
                file=sys.stderr,
                flush=True,
            )
            traceback.print_exc(file=sys.stderr)
            self.fail_pending_answer(context, error)
        return False

    def on_remote_description_set(
        self,
        _promise: Gst.Promise,
        context: dict[str, object] | None,
    ) -> None:
        promise = Gst.Promise.new_with_change_func(self.on_answer_created, context)
        self.webrtc.emit("create-answer", None, promise)

    def on_answer_created(self, promise: Gst.Promise, context: dict[str, object] | None) -> None:
        reply = promise.get_reply()
        if reply is None:
            self.fail_pending_answer(
                context,
                RuntimeError("webrtcbin create-answer returned no promise reply"),
            )
            return
        answer = reply.get_value("answer")
        if answer is None:
            print(
                f"webrtcbin create-answer reply did not contain an answer: {reply.to_string()}",
                file=sys.stderr,
                flush=True,
            )
            self.fail_pending_answer(
                context,
                RuntimeError("webrtcbin did not return an SDP answer"),
            )
            return
        set_local = Gst.Promise.new()
        self.webrtc.emit("set-local-description", answer, set_local)
        set_local.interrupt()
        self.start_media_flow()
        self.pending_answer_context = context
        self.ice_timeout_id = GLib.timeout_add_seconds(2, self.write_current_answer)
        self.write_answer_if_ice_complete()

    def on_ice_gathering_state(self, _webrtc: Gst.Element, _param: object) -> None:
        self.log_webrtc_property("ice-gathering-state")
        self.write_answer_if_ice_complete()

    def on_webrtc_state_changed(self, _webrtc: Gst.Element, param: object) -> None:
        property_name = getattr(param, "name", "")
        if property_name:
            self.log_webrtc_property(property_name)

    def log_webrtc_property(self, property_name: str) -> None:
        try:
            value = self.webrtc.get_property(property_name)
        except TypeError:
            return
        display = getattr(value, "value_nick", value)
        print(f"WebRTC {property_name}: {display}", file=sys.stderr, flush=True)

    def on_ice_candidate(self, _webrtc: Gst.Element, mline_index: int, candidate: str) -> None:
        if self.args.verbose:
            print(f"ICE candidate mline={mline_index}: {candidate}", file=sys.stderr)

    def write_answer_if_ice_complete(self) -> None:
        state = self.webrtc.get_property("ice-gathering-state")
        if state == GstWebRTC.WebRTCICEGatheringState.COMPLETE:
            self.write_current_answer()

    def write_current_answer(self) -> bool:
        description = self.webrtc.get_property("local-description")
        if description is None:
            return True
        answer_text = normalize_sdp(description.sdp.as_text())
        context = self.pending_answer_context
        if context is not None:
            context["answer"] = answer_text
            event = context.get("event")
            if isinstance(event, threading.Event):
                event.set()
            self.pending_answer_context = None
        elif self.args.answer_file:
            Path(self.args.answer_file).write_text(answer_text, encoding="utf-8")
        else:
            print("-----BEGIN 4PLAY WEBRTC SDP ANSWER-----")
            print(answer_text.rstrip())
            print("-----END 4PLAY WEBRTC SDP ANSWER-----")

        if self.ice_timeout_id is not None:
            GLib.source_remove(self.ice_timeout_id)
            self.ice_timeout_id = None
        return False

    def fail_pending_answer(
        self,
        context: dict[str, object] | None,
        error: Exception,
    ) -> None:
        if context is None:
            fail(str(error))
        context["error"] = error
        event = context.get("event")
        if isinstance(event, threading.Event):
            event.set()

    def on_bus_message(self, _bus: Gst.Bus, message: Gst.Message) -> None:
        if message.type == Gst.MessageType.ERROR:
            error, debug = message.parse_error()
            fail(f"GStreamer error: {error.message}\n{debug or ''}")
        if message.type == Gst.MessageType.WARNING:
            warning, debug = message.parse_warning()
            print(
                f"GStreamer warning from {message.src.get_name()}: {warning.message}\n{debug or ''}",
                file=sys.stderr,
                flush=True,
            )
        if message.type == Gst.MessageType.STATE_CHANGED and message.src in (self.pipeline, self.webrtc):
            old, new, pending = message.parse_state_changed()
            print(
                f"GStreamer state {message.src.get_name()}: "
                f"{old.value_nick}->{new.value_nick} pending={pending.value_nick}",
                file=sys.stderr,
                flush=True,
            )
        if message.type == Gst.MessageType.EOS:
            print(
                f"Pipeline ended after EOS from {message.src.get_name()}.",
                file=sys.stderr,
                flush=True,
            )
            self.loop.quit()


def request_handler(publisher: RawSessionPublisher) -> type[BaseHTTPRequestHandler]:
    class RawSessionHandler(BaseHTTPRequestHandler):
        server_version = "4PlayRawSessionWebRtc/0.1"

        def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API.
            if self.path not in ("/", "/health"):
                self.send_error(404, "not found")
                return
            self.send_response(200)
            self.send_cors_headers()
            self.send_header("content-type", "text/plain; charset=utf-8")
            self.end_headers()
            self.wfile.write(
                f"4-Play raw session WebRTC publisher ready for {publisher.args.session_id}\n".encode(
                    "utf-8",
                )
            )

        def do_OPTIONS(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API.
            self.send_response(204)
            self.send_cors_headers()
            self.end_headers()

        def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API.
            valid_paths = {
                "/offer",
                f"/sessions/{publisher.args.session_id}/offer",
            }
            if self.path not in valid_paths:
                self.send_error(404, "not found")
                return
            try:
                length = int(self.headers.get("content-length", "0"))
                offer = self.rfile.read(length).decode("utf-8")
                print(
                    f"Received SDP offer path={self.path} bytes={len(offer)} "
                    f"client={self.client_address[0]}",
                    file=sys.stderr,
                    flush=True,
                )
                browser_host_ip = publisher.args.browser_host_ip or self.client_address[0]
                answer = publisher.answer_for_offer(offer, browser_host_ip)
            except Exception as error:  # noqa: BLE001 - diagnostic publisher returns text.
                print(
                    f"Failed to answer SDP offer path={self.path}: {error}",
                    file=sys.stderr,
                    flush=True,
                )
                traceback.print_exc(file=sys.stderr)
                self.send_response(500)
                self.send_cors_headers()
                self.send_header("content-type", "text/plain; charset=utf-8")
                self.end_headers()
                self.wfile.write(str(error).encode("utf-8"))
                return

            self.send_response(200)
            self.send_cors_headers()
            self.send_header("content-type", "application/sdp; charset=utf-8")
            self.end_headers()
            self.wfile.write(answer.encode("utf-8"))

        def log_message(self, format: str, *args: object) -> None:
            print(f"{self.client_address[0]} - {format % args}", file=sys.stderr)

        def send_cors_headers(self) -> None:
            self.send_header("access-control-allow-origin", "*")
            self.send_header("access-control-allow-methods", "GET, POST, OPTIONS")
            self.send_header("access-control-allow-headers", "content-type")

    return RawSessionHandler


def normalize_sdp(text: str) -> str:
    normalized_lines: list[str] = []
    for raw_line in text.replace("\ufeff", "").splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if line.startswith("a=fingerprint:"):
            prefix, _, fingerprint = line.partition(" ")
            line = f"{prefix.lower()} {fingerprint.lower()}"
        normalized_lines.append(line)
    return "\r\n".join(normalized_lines) + "\r\n"


def replace_mdns_candidates(text: str, browser_host_ip: str) -> str:
    output_lines: list[str] = []
    replacements = 0
    for line in text.splitlines():
        if line.startswith("a=candidate:"):
            parts = line.split(" ")
            if len(parts) >= 5 and parts[4].endswith(".local"):
                parts[4] = browser_host_ip
                line = " ".join(parts)
                replacements += 1
        output_lines.append(line)
    if replacements:
        print(
            f"Replaced {replacements} browser mDNS ICE candidate(s) with {browser_host_ip}.",
            file=sys.stderr,
        )
    return "\n".join(output_lines) + "\n"


def quote_path(path: str) -> str:
    if any(character.isspace() for character in path):
        return repr(path)
    return path


def install_signal_handlers(loop: GLib.MainLoop) -> None:
    def stop(_signum: int, _frame: object) -> None:
        loop.quit()

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)


def fail(message: str) -> NoReturn:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--session-id", required=True)
    parser.add_argument("--video-path", required=True)
    parser.add_argument("--audio-path", required=True)
    parser.add_argument("--width", type=int, required=True)
    parser.add_argument("--height", type=int, required=True)
    parser.add_argument("--fps-num", type=int, default=60)
    parser.add_argument("--fps-den", type=int, default=1)
    parser.add_argument("--http-signal", default="127.0.0.1:43000")
    parser.add_argument("--answer-file")
    parser.add_argument("--video-bitrate-kbps", type=int, default=1400)
    parser.add_argument("--audio-bitrate-bps", type=int, default=64000)
    parser.add_argument("--keyframe-distance", type=int, default=15)
    parser.add_argument(
        "--browser-host-ip",
        default=os.environ.get("FOURPLAY_WEBRTC_BROWSER_HOST_IP"),
        help="Replace browser .local ICE candidates with this LAN IP.",
    )
    parser.add_argument("--verbose", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    Gst.init(None)
    RawSessionPublisher(args).run_http_signaling()


if __name__ == "__main__":
    main()
