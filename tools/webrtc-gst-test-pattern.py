#!/usr/bin/env python3
"""
Manual WebRTC publisher spike for 4-Play browser-native media.

This script intentionally uses copy/paste SDP signaling so the first latency
experiment stays independent from the production control plane. It publishes a
low-latency GStreamer test pattern to the browser `/webrtc-test` page.
"""

from __future__ import annotations

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import signal
import sys
import threading
from pathlib import Path
from typing import NoReturn

import gi

gi.require_version("Gst", "1.0")
gi.require_version("GstSdp", "1.0")
gi.require_version("GstWebRTC", "1.0")

from gi.repository import GLib, Gst, GstSdp, GstWebRTC  # noqa: E402


PIPELINE_TEMPLATE = """
webrtcbin name=webrtc bundle-policy=max-bundle latency=0
videotestsrc is-live=true pattern={pattern}
  ! video/x-raw,width={width},height={height},framerate={fps}/1
  ! videoconvert
  ! queue leaky=downstream max-size-buffers=2 max-size-time=0 max-size-bytes=0
  ! {video_encoder}
  ! {video_payloader}
  ! application/x-rtp,media=video,encoding-name={video_encoding},payload=96,clock-rate=90000
  ! webrtc.
{audio_pipeline}
"""

AUDIO_PIPELINE_TEMPLATE = """
audiotestsrc is-live=true wave=ticks tick-interval=1000000000
  ! audio/x-raw,rate=48000,channels=2
  ! audioconvert
  ! audioresample
  ! queue leaky=downstream max-size-buffers=4 max-size-time=0 max-size-bytes=0
  ! opusenc audio-type=restricted-lowdelay bitrate={audio_bitrate_bps}
  ! rtpopuspay pt={audio_payload_type}
  ! application/x-rtp,media=audio,encoding-name=OPUS,payload={audio_payload_type},clock-rate=48000
  ! webrtc.
"""


class WebRtcTestPatternPublisher:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.loop = GLib.MainLoop()
        self.answer_written = False
        self.started = False
        self.ice_timeout_id: int | None = None
        self.pending_answer_context: dict[str, object] | None = None
        video_encoder, video_payloader, video_encoding = video_pipeline(args)
        audio_pipeline = ""
        if not args.video_only:
            audio_pipeline = AUDIO_PIPELINE_TEMPLATE.format(
                audio_bitrate_bps=args.audio_bitrate_bps,
                audio_payload_type=args.audio_payload_type,
            )
        self.pipeline = Gst.parse_launch(
            PIPELINE_TEMPLATE.format(
                pattern=args.pattern,
                width=args.width,
                height=args.height,
                fps=args.fps,
                video_encoder=video_encoder,
                video_payloader=video_payloader,
                video_encoding=video_encoding,
                audio_pipeline=audio_pipeline,
            )
        )
        self.webrtc = self.pipeline.get_by_name("webrtc")
        if self.webrtc is None:
            raise RuntimeError("pipeline did not create webrtcbin named 'webrtc'")

        self.webrtc.connect("notify::ice-gathering-state", self.on_ice_gathering_state)
        self.webrtc.connect("on-ice-candidate", self.on_ice_candidate)

        bus = self.pipeline.get_bus()
        bus.add_signal_watch()
        bus.connect("message", self.on_bus_message)

    def run(self) -> None:
        offer_text = read_offer(self.args.offer_file)
        if self.args.browser_host_ip:
            offer_text = replace_mdns_candidates(offer_text, self.args.browser_host_ip)
        self.start_pipeline()
        self.apply_offer_and_create_answer(offer_text)
        install_signal_handlers(self.loop)
        print("Publisher running. Press Ctrl-C after applying the answer in the browser.", file=sys.stderr)
        self.loop.run()
        self.pipeline.set_state(Gst.State.NULL)

    def run_http_signaling(self) -> None:
        host, port_text = self.args.http_signal.rsplit(":", 1)
        server = ThreadingHTTPServer((host, int(port_text)), publisher_request_handler(self))
        thread = threading.Thread(
            target=server.serve_forever,
            name="4play-webrtc-spike-http",
            daemon=True,
        )
        thread.start()
        install_signal_handlers(self.loop)
        print(
            f"WebRTC spike signaling server listening at http://{host}:{port_text}/offer",
            file=sys.stderr,
        )
        print("Use this URL from /webrtc-test. Press Ctrl-C to stop.", file=sys.stderr)
        try:
            self.loop.run()
        finally:
            server.shutdown()
            self.pipeline.set_state(Gst.State.NULL)

    def start_pipeline(self) -> None:
        if not self.started:
            self.pipeline.set_state(Gst.State.PLAYING)
            self.started = True

    def answer_for_offer(
        self,
        offer_text: str,
        browser_host_ip: str | None,
        timeout_seconds: float = 8.0,
    ) -> str:
        if browser_host_ip:
            offer_text = replace_mdns_candidates(offer_text, browser_host_ip)

        event = threading.Event()
        context: dict[str, object] = {
            "event": event,
        }
        GLib.idle_add(self.apply_offer_and_create_answer, offer_text, context)
        if not event.wait(timeout_seconds):
            raise RuntimeError("timed out waiting for GStreamer SDP answer")
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
        self.start_pipeline()
        try:
            result, sdp_message = GstSdp.SDPMessage.new_from_text(offer_text)
            if result != GstSdp.SDPResult.OK:
                raise RuntimeError(f"failed to parse browser SDP offer: {result}")

            offer = GstWebRTC.WebRTCSessionDescription.new(
                GstWebRTC.WebRTCSDPType.OFFER,
                sdp_message,
            )
            promise = Gst.Promise.new_with_change_func(self.on_remote_description_set, context)
            self.webrtc.emit("set-remote-description", offer, promise)
        except Exception as error:  # noqa: BLE001 - spike tool reports any setup failure.
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
        answer = reply.get_value("answer")
        if answer is None:
            self.fail_pending_answer(context, RuntimeError("webrtcbin did not return an SDP answer"))
            return

        set_local = Gst.Promise.new()
        self.webrtc.emit("set-local-description", answer, set_local)
        set_local.interrupt()

        # Give ICE a short chance to place host candidates into the local
        # description before writing the copy/paste answer. If gathering is
        # already complete, this immediately writes the answer.
        self.pending_answer_context = context
        self.ice_timeout_id = GLib.timeout_add_seconds(2, self.write_current_answer)
        self.write_answer_if_ice_complete()

    def on_ice_gathering_state(self, _webrtc: Gst.Element, _param: object) -> None:
        self.write_answer_if_ice_complete()

    def on_ice_candidate(self, _webrtc: Gst.Element, mline_index: int, candidate: str) -> None:
        if self.args.verbose:
            print(f"ICE candidate mline={mline_index}: {candidate}", file=sys.stderr)

    def write_answer_if_ice_complete(self) -> None:
        state = self.webrtc.get_property("ice-gathering-state")
        if self.args.verbose:
            print(f"ICE gathering state: {state.value_nick}", file=sys.stderr)
        if state == GstWebRTC.WebRTCICEGatheringState.COMPLETE:
            self.write_current_answer()

    def write_current_answer(self) -> bool:
        if self.answer_written and self.pending_answer_context is None:
            return False
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
            print(f"Wrote SDP answer to {self.args.answer_file}", file=sys.stderr)
        else:
            print("-----BEGIN 4PLAY WEBRTC SDP ANSWER-----")
            print(answer_text.rstrip())
            print("-----END 4PLAY WEBRTC SDP ANSWER-----")
            print("Paste only the SDP lines between the markers into /webrtc-test.", file=sys.stderr)

        self.answer_written = True
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
        if message.type == Gst.MessageType.EOS:
            print("Pipeline ended.", file=sys.stderr)
            self.loop.quit()


def read_offer(path: str | None) -> str:
    if path:
        text = Path(path).read_text(encoding="utf-8")
    else:
        print("Paste the browser SDP offer, then press Ctrl-D:", file=sys.stderr)
        text = sys.stdin.read()
    text = text.strip()
    if "v=0" not in text or "m=" not in text:
        raise RuntimeError("SDP offer does not look valid")
    return text


def normalize_sdp(text: str) -> str:
    """Normalize SDP for browser copy/paste parsing.

    Some terminals and copy/paste paths can preserve odd whitespace or casing.
    Browsers are stricter than GStreamer in a few places, so keep the generated
    answer conservative: no empty copied lines, CRLF line endings, and lowercase
    DTLS fingerprint algorithm/hash text.
    """
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
    """Replace browser mDNS ICE candidate hostnames with a LAN IP.

    Chrome/Edge often hide local IPs behind `.local` mDNS candidates. That is
    correct browser behavior, but the manual copy/paste spike can fail if the
    Linux publisher cannot resolve those names. This spike-only helper lets us
    prove the media path with a known browser host IP before we build proper
    signaling/trickle ICE.
    """
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


def publisher_request_handler(
    publisher: WebRtcTestPatternPublisher,
) -> type[BaseHTTPRequestHandler]:
    class WebRtcSpikeHandler(BaseHTTPRequestHandler):
        server_version = "4PlayWebRtcSpike/0.1"

        def do_OPTIONS(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API.
            self.send_response(204)
            self.send_cors_headers()
            self.end_headers()

        def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API.
            if self.path not in ("/", "/health"):
                self.send_error(404, "not found")
                return
            self.send_response(200)
            self.send_cors_headers()
            self.send_header("content-type", "text/plain; charset=utf-8")
            self.end_headers()
            self.wfile.write(b"4-Play WebRTC spike publisher ready\n")

        def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API.
            if self.path != "/offer":
                self.send_error(404, "not found")
                return
            try:
                length = int(self.headers.get("content-length", "0"))
                offer = self.rfile.read(length).decode("utf-8")
                browser_host_ip = publisher.args.browser_host_ip or self.client_address[0]
                answer = publisher.answer_for_offer(offer, browser_host_ip)
            except Exception as error:  # noqa: BLE001 - spike server returns diagnostic text.
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

    return WebRtcSpikeHandler


def video_pipeline(args: argparse.Namespace) -> tuple[str, str, str]:
    if args.codec == "vp8":
        encoder = (
            f"vp8enc deadline=1 cpu-used=8 target-bitrate={args.video_bitrate_kbps * 1000} "
            f"keyframe-max-dist={max(1, args.fps // 2)}"
        )
        return encoder, "rtpvp8pay pt=96", "VP8"
    if args.codec == "h264":
        encoder = (
            "x264enc tune=zerolatency speed-preset=ultrafast "
            f"bitrate={args.video_bitrate_kbps} key-int-max={max(1, args.fps // 4)} "
            "bframes=0 byte-stream=true ! video/x-h264,profile=baseline "
            "! h264parse config-interval=-1"
        )
        return encoder, "rtph264pay pt=96 config-interval=-1", "H264"
    raise RuntimeError(f"unsupported codec: {args.codec}")


def install_signal_handlers(loop: GLib.MainLoop) -> None:
    def stop(_signum: int, _frame: object) -> None:
        loop.quit()

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)


def fail(message: str) -> NoReturn:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Publish a low-latency GStreamer WebRTC test pattern for /webrtc-test.",
    )
    parser.add_argument("--offer-file", help="Path containing the browser SDP offer.")
    parser.add_argument("--answer-file", help="Write the generated SDP answer to this path.")
    parser.add_argument(
        "--http-signal",
        help=(
            "Serve a spike HTTP signaling endpoint at host:port, for example "
            "0.0.0.0:18080. The browser can POST an SDP offer to /offer."
        ),
    )
    parser.add_argument("--width", type=int, default=320)
    parser.add_argument("--height", type=int, default=224)
    parser.add_argument("--fps", type=int, default=60)
    parser.add_argument("--pattern", default="ball")
    parser.add_argument(
        "--browser-host-ip",
        help=(
            "Spike-only workaround: replace browser .local ICE candidates in the "
            "offer with this LAN IP, for example the Windows seat IP."
        ),
    )
    parser.add_argument(
        "--codec",
        choices=["vp8", "h264"],
        default="vp8",
        help="Video codec for the test publisher. VP8 is the default because browsers usually offer it first.",
    )
    parser.add_argument(
        "--video-only",
        action="store_true",
        help="Publish only video. This is the recommended first connectivity test.",
    )
    parser.add_argument("--video-bitrate-kbps", type=int, default=1400)
    parser.add_argument("--audio-bitrate-bps", type=int, default=64000)
    parser.add_argument(
        "--audio-payload-type",
        type=int,
        default=111,
        help=(
            "RTP payload type for Opus audio. Browsers normally offer Opus as "
            "dynamic payload type 111, so 111 is the default."
        ),
    )
    parser.add_argument("--verbose", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    Gst.init(None)
    publisher = WebRtcTestPatternPublisher(args)
    if args.http_signal:
        publisher.run_http_signaling()
    else:
        publisher.run()


if __name__ == "__main__":
    main()
