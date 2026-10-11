#!/usr/bin/env python3
"""
Manual WebRTC publisher spike for 4-Play browser-native media.

This script intentionally uses copy/paste SDP signaling so the first latency
experiment stays independent from the production control plane. It publishes a
low-latency GStreamer test pattern to the browser `/webrtc-test` page.
"""

from __future__ import annotations

import argparse
import signal
import sys
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
  ! x264enc tune=zerolatency speed-preset=ultrafast bitrate={video_bitrate_kbps}
      key-int-max={key_interval} bframes=0 byte-stream=true
  ! video/x-h264,profile=baseline
  ! h264parse config-interval=-1
  ! rtph264pay pt=96 config-interval=-1
  ! application/x-rtp,media=video,encoding-name=H264,payload=96,clock-rate=90000
  ! webrtc.
audiotestsrc is-live=true wave=ticks tick-interval=1000000000
  ! audio/x-raw,rate=48000,channels=2
  ! audioconvert
  ! audioresample
  ! queue leaky=downstream max-size-buffers=4 max-size-time=0 max-size-bytes=0
  ! opusenc audio-type=restricted-lowdelay bitrate={audio_bitrate_bps}
  ! rtpopuspay pt=97
  ! application/x-rtp,media=audio,encoding-name=OPUS,payload=97,clock-rate=48000
  ! webrtc.
"""


class WebRtcTestPatternPublisher:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.loop = GLib.MainLoop()
        self.answer_written = False
        self.ice_timeout_id: int | None = None
        self.pipeline = Gst.parse_launch(
            PIPELINE_TEMPLATE.format(
                pattern=args.pattern,
                width=args.width,
                height=args.height,
                fps=args.fps,
                video_bitrate_kbps=args.video_bitrate_kbps,
                key_interval=max(1, args.fps // 4),
                audio_bitrate_bps=args.audio_bitrate_bps,
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
        self.pipeline.set_state(Gst.State.PLAYING)
        self.apply_offer_and_create_answer(offer_text)
        install_signal_handlers(self.loop)
        print("Publisher running. Press Ctrl-C after applying the answer in the browser.", file=sys.stderr)
        self.loop.run()
        self.pipeline.set_state(Gst.State.NULL)

    def apply_offer_and_create_answer(self, offer_text: str) -> None:
        result, sdp_message = GstSdp.SDPMessage.new_from_text(offer_text)
        if result != GstSdp.SDPResult.OK:
            raise RuntimeError(f"failed to parse browser SDP offer: {result}")

        offer = GstWebRTC.WebRTCSessionDescription.new(
            GstWebRTC.WebRTCSDPType.OFFER,
            sdp_message,
        )
        promise = Gst.Promise.new()
        self.webrtc.emit("set-remote-description", offer, promise)
        promise.interrupt()

        promise = Gst.Promise.new_with_change_func(self.on_answer_created, None)
        self.webrtc.emit("create-answer", None, promise)

    def on_answer_created(self, promise: Gst.Promise, _user_data: object) -> None:
        reply = promise.get_reply()
        answer = reply.get_value("answer")
        if answer is None:
            fail("webrtcbin did not return an SDP answer")

        set_local = Gst.Promise.new()
        self.webrtc.emit("set-local-description", answer, set_local)
        set_local.interrupt()

        # Give ICE a short chance to place host candidates into the local
        # description before writing the copy/paste answer. If gathering is
        # already complete, this immediately writes the answer.
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
        if self.answer_written:
            return False
        description = self.webrtc.get_property("local-description")
        if description is None:
            return True

        answer_text = description.sdp.as_text()
        if self.args.answer_file:
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
    parser.add_argument("--width", type=int, default=320)
    parser.add_argument("--height", type=int, default=224)
    parser.add_argument("--fps", type=int, default=60)
    parser.add_argument("--pattern", default="ball")
    parser.add_argument("--video-bitrate-kbps", type=int, default=1400)
    parser.add_argument("--audio-bitrate-bps", type=int, default=64000)
    parser.add_argument("--verbose", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    Gst.init(None)
    publisher = WebRtcTestPatternPublisher(args)
    publisher.run()


if __name__ == "__main__":
    main()
