#!/usr/bin/env python3
"""
Experimental WebRTC publisher for a single 4-Play MAME session's raw FIFOs.

This is the first bridge from real MAME output to browser-native media:

    session-runtime video.raw/audio.pcm FIFOs
      -> complete frames / shared-clock timestamps / GStreamer appsrc encoders
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
import select
import signal
import sys
import threading
import time
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
appsrc name=video_source is-live=true format=time block=false max-buffers=2 max-bytes=0 max-time=0 leaky-type=downstream
  caps="video/x-raw,format=BGRx,width={width},height={height},framerate={fps_num}/{fps_den}"
  ! queue name=video_queue leaky=downstream max-size-buffers=2 max-size-time=0 max-size-bytes=0
  ! videoconvert
  ! video/x-raw,format=I420
  ! vp8enc deadline=1 cpu-used=8 target-bitrate={video_bitrate_bps} keyframe-max-dist={keyframe_distance}
  ! rtpvp8pay pt=96
  ! application/x-rtp,media=video,encoding-name=VP8,payload=96,clock-rate=90000
  ! webrtc.
appsrc name=audio_source is-live=true format=time block=false max-buffers=4 max-bytes=0 max-time=0 leaky-type=downstream
  caps="audio/x-raw,format=S16LE,rate=48000,channels=2,layout=interleaved"
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
        self.offer_lock = threading.Lock()
        self.media_lock = threading.Lock()
        self.reader_stop = threading.Event()
        self.reader_threads: list[threading.Thread] = []
        self.media_epoch_ns: int | None = None
        self.last_pts: dict[str, int] = {}
        self.started = False
        self.media_started = False
        self.pipeline_failed = False
        self.pending_answer_context: dict[str, object] | None = None
        self.ice_timeout_id: int | None = None
        self.pipeline: Gst.Pipeline | None = None
        self.webrtc: Gst.Element | None = None

        self.build_pipeline()

    def build_pipeline(self) -> None:
        self.pipeline = Gst.parse_launch(
            PIPELINE_TEMPLATE.format(
                video_path=quote_path(self.args.video_path),
                audio_path=quote_path(self.args.audio_path),
                width=self.args.width,
                height=self.args.height,
                fps_num=self.args.fps_num,
                fps_den=self.args.fps_den,
                video_bitrate_bps=self.args.video_bitrate_kbps * 1000,
                keyframe_distance=self.args.keyframe_distance,
                audio_bitrate_bps=self.args.audio_bitrate_bps,
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

    def reset_pipeline(self, reason: str) -> None:
        print(
            f"Resetting raw-session WebRTC pipeline session={self.args.session_id}: {reason}",
            file=sys.stderr,
            flush=True,
        )
        if self.ice_timeout_id is not None:
            GLib.source_remove(self.ice_timeout_id)
            self.ice_timeout_id = None
        with self.media_lock:
            self.media_epoch_ns = None
        if self.pipeline is not None:
            self.pipeline.get_bus().remove_signal_watch()
            self.pipeline.set_state(Gst.State.NULL)
        self.started = False
        self.media_started = False
        self.pipeline_failed = False
        self.pending_answer_context = None
        self.build_pipeline()

    def run_http_signaling(self) -> None:
        # Keep FIFO readers open across peer replacement. Closing halfway through
        # a frame would leave its remainder in the next reader's first frame.
        for label, path, size in (
            ("video", self.args.video_path, self.args.width * self.args.height * 4),
            ("audio", self.args.audio_path, 960 * 2 * 2),
        ):
            thread = threading.Thread(
                target=self.read_fifo, args=(label, path, size), daemon=True,
                name=f"4play-webrtc-{label}-reader",
            )
            self.reader_threads.append(thread)
            thread.start()
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
            self.reader_stop.set()
            with self.media_lock:
                self.media_epoch_ns = None
            server.shutdown()
            for reader in self.reader_threads:
                reader.join(timeout=1)
            if self.pipeline is not None:
                self.pipeline.set_state(Gst.State.NULL)

    def read_fifo(self, label: str, path: str, block_bytes: int) -> None:
        try:
            fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
            try:
                pending = bytearray()
                while not self.reader_stop.is_set():
                    if not select.select([fd], [], [], 0.05)[0]:
                        continue
                    try:
                        chunk = os.read(fd, block_bytes - len(pending))
                    except BlockingIOError:
                        continue
                    if not chunk:
                        # No writer yet (or writer reconnecting), not media EOS.
                        self.reader_stop.wait(0.01)
                        continue
                    pending.extend(chunk)
                    if len(pending) == block_bytes:
                        self.push_media_block(label, pending)
                        pending.clear()
            finally:
                os.close(fd)
        except Exception as error:
            print(f"Raw {label} FIFO reader failed: {error}", file=sys.stderr, flush=True)
            GLib.idle_add(self.mark_pipeline_failed, f"{label} reader: {error}")

    def push_media_block(self, label: str, data: bytearray) -> None:
        with self.media_lock:
            if self.media_epoch_ns is None or self.pipeline is None:
                return  # Drain stale frames even when no browser is connected.
            pts = max(time.monotonic_ns() - self.media_epoch_ns, self.last_pts.get(label, -1) + 1)
            self.last_pts[label] = pts
            buffer = Gst.Buffer.new_allocate(None, len(data), None)
            buffer.fill(0, bytes(data))
            buffer.pts = pts
            buffer.dts = Gst.CLOCK_TIME_NONE
            buffer.duration = (
                Gst.SECOND * self.args.fps_den // self.args.fps_num
                if label == "video" else Gst.SECOND // 50
            )
            source = self.pipeline.get_by_name(f"{label}_source")
            result = source.emit("push-buffer", buffer)
            if result not in (Gst.FlowReturn.OK, Gst.FlowReturn.FLUSHING):
                self.media_epoch_ns = None
                GLib.idle_add(self.mark_pipeline_failed, f"{label} push-buffer: {result.value_nick}")

    def start_pipeline(self) -> None:
        if self.started:
            return
        if self.pipeline is None or self.webrtc is None:
            self.build_pipeline()
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
        if self.pipeline is None:
            raise RuntimeError("GStreamer pipeline is not initialized")
        print(
            f"Starting raw-session WebRTC media flow session={self.args.session_id}",
            file=sys.stderr,
            flush=True,
        )
        self.pipeline.set_state(Gst.State.PLAYING)
        with self.media_lock:
            self.last_pts.clear()
            self.media_epoch_ns = time.monotonic_ns()
        self.media_started = True

    def answer_for_offer(
        self,
        offer_text: str,
        browser_host_ip: str | None,
        timeout_seconds: float = 8.0,
    ) -> str:
        with self.offer_lock:
            if self.pipeline_failed or self.media_started:
                self.reset_pipeline("new browser offer")
            if browser_host_ip:
                offer_text = replace_mdns_candidates(offer_text, browser_host_ip)
            event = threading.Event()
            context: dict[str, object] = {"event": event}
            GLib.idle_add(self.apply_offer_and_create_answer, offer_text, context)
            if not event.wait(timeout_seconds):
                if self.pipeline is not None:
                    state = self.pipeline.get_state(0)
                    state_text = f"{state.state.value_nick} pending={state.pending.value_nick}"
                else:
                    state_text = "not-initialized"
                raise RuntimeError(
                    "timed out waiting for GStreamer SDP answer; "
                    f"pipeline_state={state_text}"
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
            if self.webrtc is None:
                raise RuntimeError("GStreamer WebRTC element is not initialized")
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
        if self.webrtc is None:
            self.fail_pending_answer(
                context,
                RuntimeError("GStreamer WebRTC element is not initialized"),
            )
            return
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
        if self.webrtc is None:
            self.fail_pending_answer(
                context,
                RuntimeError("GStreamer WebRTC element is not initialized"),
            )
            return
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
            if self.webrtc is None:
                return
            value = self.webrtc.get_property(property_name)
        except TypeError:
            return
        display = getattr(value, "value_nick", value)
        print(f"WebRTC {property_name}: {display}", file=sys.stderr, flush=True)

    def on_ice_candidate(self, _webrtc: Gst.Element, mline_index: int, candidate: str) -> None:
        if self.args.verbose:
            print(f"ICE candidate mline={mline_index}: {candidate}", file=sys.stderr)

    def write_answer_if_ice_complete(self) -> None:
        if self.webrtc is None:
            return
        state = self.webrtc.get_property("ice-gathering-state")
        if state == GstWebRTC.WebRTCICEGatheringState.COMPLETE:
            self.write_current_answer()

    def write_current_answer(self) -> bool:
        if self.webrtc is None:
            return True
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

    def mark_pipeline_failed(self, reason: str) -> None:
        print(
            f"Raw-session WebRTC pipeline stopped session={self.args.session_id}: {reason}",
            file=sys.stderr,
            flush=True,
        )
        self.pipeline_failed = True
        self.media_started = False
        with self.media_lock:
            self.media_epoch_ns = None
        if self.pending_answer_context is not None:
            self.fail_pending_answer(self.pending_answer_context, RuntimeError(reason))
            self.pending_answer_context = None
        if self.pipeline is not None:
            self.pipeline.set_state(Gst.State.NULL)

    def on_bus_message(self, _bus: Gst.Bus, message: Gst.Message) -> None:
        if message.type == Gst.MessageType.ERROR:
            error, debug = message.parse_error()
            self.mark_pipeline_failed(f"GStreamer error from {message.src.get_name()}: {error.message}\n{debug or ''}")
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
            self.mark_pipeline_failed(f"EOS from {message.src.get_name()}")


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
