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
UDP/FFplay path. Browser connections have independent bounded pipelines; the
shared FIFO reader never splits frames among viewers.
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
from webrtc_browser_input import BrowserInputRelay, InputAuthorization
from webrtc_peer_pool import PeerPool, PeerCapacityError
from webrtc_client_address import browser_host_for_request
from webrtc_ice_ports import IceUdpPortRange, add_ice_port_arguments, configure_ice_udp_ports, validate_answer_candidates

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
    def __init__(self, args: argparse.Namespace, input_relay=None) -> None:
        self.args = args
        self.ice_ports = IceUdpPortRange.from_args(args)
        self.loop = GLib.MainLoop()
        self.offer_lock = threading.Lock()
        self.owns_input_relay = input_relay is None
        self.input_relay = input_relay or BrowserInputRelay()
        self.input_authorization = None
        self.closed = False
        self.created_at = time.monotonic()
        self.unhealthy_since = None
        self.input_channel = None
        self.input_channel_handlers = []
        self.webrtc_handlers = []
        self.bus_handler = None
        self.media_lock = threading.Lock()
        self.reader_stop = threading.Event()
        self.reader_threads: list[threading.Thread] = []
        self.media_epoch_ns: int | None = None
        self.last_pts: dict[str, int] = {}
        self.audio_next_pts: int | None = None
        self.started = False
        self.media_started = False
        self.pipeline_failed = False
        self.pending_answer_context: dict[str, object] | None = None
        self.ice_timeout_id: int | None = None
        self.pipeline: Gst.Pipeline | None = None
        self.webrtc: Gst.Element | None = None

        self.build_pipeline()

    def close(self) -> None:
        if self.closed:
            return
        self.closed = True
        if self.ice_timeout_id is not None:
            GLib.source_remove(self.ice_timeout_id)
            self.ice_timeout_id = None
        with self.media_lock:
            self.media_epoch_ns = None
        self.detach_callbacks()
        if self.input_authorization:
            self.input_relay.revoke(self.input_authorization.peer_id)
        if self.owns_input_relay:
            self.input_relay.close()
        if self.pending_answer_context is not None:
            self.fail_pending_answer(self.pending_answer_context, RuntimeError("browser peer closed"))
            self.pending_answer_context = None
        if self.pipeline is not None:
            self.pipeline.get_bus().remove_signal_watch()
            self.pipeline.set_state(Gst.State.NULL)

    def detach_callbacks(self):
        if self.input_channel is not None:
            for handler in self.input_channel_handlers:
                self.input_channel.disconnect(handler)
            self.input_channel_handlers = []
            self.input_channel.emit("close")
            self.input_channel = None
        if self.webrtc is not None:
            for handler in self.webrtc_handlers:
                self.webrtc.disconnect(handler)
            self.webrtc_handlers = []
        if self.pipeline is not None and self.bus_handler is not None:
            self.pipeline.get_bus().disconnect(self.bus_handler)
            self.bus_handler = None

    def renew_input(self, authorization):
        return self.input_relay.renew(authorization)

    def expired(self, now: float) -> bool:
        if self.closed or self.pipeline_failed:
            return True
        if self.input_authorization:
            current = self.input_relay.authorization
            if current is None or current.peer_id != self.input_authorization.peer_id:
                return True
            if current.expires_ms <= time.time_ns() // 1_000_000:
                return True
        if self.webrtc is None:
            return now - self.created_at >= 30
        state = self.webrtc.get_property("connection-state").value_nick
        if state in ("failed", "closed"):
            return True
        if state == "connected":
            self.unhealthy_since = None
            return False
        if state == "disconnected":
            self.unhealthy_since = self.unhealthy_since or now
            return now - self.unhealthy_since >= 15
        return now - self.created_at >= 30

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
        configure_ice_udp_ports(self.webrtc, self.ice_ports)

        self.webrtc_handlers = [self.webrtc.connect(signal, callback) for signal, callback in (
            ("notify::ice-gathering-state", self.on_ice_gathering_state),
            ("notify::ice-connection-state", self.on_webrtc_state_changed),
            ("notify::connection-state", self.on_webrtc_state_changed),
            ("notify::signaling-state", self.on_webrtc_state_changed),
            ("on-ice-candidate", self.on_ice_candidate),
            ("on-data-channel", self.on_data_channel),
        )]

        bus = self.pipeline.get_bus()
        bus.add_signal_watch()
        self.bus_handler = bus.connect("message", self.on_bus_message)

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
        self.detach_callbacks()
        self.input_relay.authorize(None)
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
            self.input_relay.close()
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
            arrival_pts = time.monotonic_ns() - self.media_epoch_ns
            duration = (
                Gst.SECOND * self.args.fps_den // self.args.fps_num
                if label == "video" else (len(data) // 4) * Gst.SECOND // 48000
            )
            discontinuity = False
            if label == "audio":
                # PCM samples are continuous even when FIFO reads arrive in
                # bursts. Arrival-time PTS creates overlaps/gaps in Opus input.
                pts = self.audio_next_pts
                if pts is None or abs(arrival_pts - pts) > Gst.SECOND // 10:
                    if pts is not None:
                        print(
                            f"Audio clock reanchored after {(arrival_pts - pts) / 1_000_000:+.1f} ms drift",
                            file=sys.stderr, flush=True,
                        )
                    previous_pts = self.last_pts.get(label)
                    pts = max(arrival_pts, previous_pts + duration if previous_pts is not None else 0)
                    discontinuity = True
                self.audio_next_pts = pts + duration
            else:
                pts = max(arrival_pts, self.last_pts.get(label, -1) + 1)
            self.last_pts[label] = pts
            buffer = Gst.Buffer.new_allocate(None, len(data), None)
            buffer.fill(0, bytes(data))
            buffer.pts = pts
            buffer.dts = Gst.CLOCK_TIME_NONE
            buffer.duration = duration
            if discontinuity:
                buffer.set_flags(Gst.BufferFlags.DISCONT)
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
            self.audio_next_pts = None
            self.media_epoch_ns = time.monotonic_ns()
        self.media_started = True

    def answer_for_offer(
        self,
        offer_text: str,
        browser_host_ip: str | None,
        timeout_seconds: float = 8.0,
        input_authorization: InputAuthorization | None = None,
    ) -> str:
        with self.offer_lock:
            if self.closed:
                raise RuntimeError("browser peer closed")
            if self.pipeline_failed or self.media_started:
                self.reset_pipeline("new browser offer")
            self.input_authorization = input_authorization
            self.input_relay.authorize(input_authorization)
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
            validate_answer_candidates(answer, self.ice_ports)
            if self.args.answer_file:
                Path(self.args.answer_file).write_text(answer, encoding="utf-8")
            return answer

    def on_data_channel(self, _webrtc, channel):
        authorization = self.input_authorization
        if self.closed:
            channel.emit("close")
            return
        if channel.get_property("label") != "4play-input" or authorization is None:
            channel.emit("close")
            return
        if self.input_channel is not None:
            channel.emit("close")
            return
        self.input_channel = channel
        peer_id = authorization.peer_id

        def receive(_channel, message):
            if self.input_channel is channel:
                self.input_relay.receive(message, peer_id)

        def closed(_channel):
            if self.input_channel is channel:
                self.input_relay.neutralize(peer_id)
                for handler in self.input_channel_handlers:
                    channel.disconnect(handler)
                self.input_channel_handlers = []
                self.input_channel = None

        self.input_channel_handlers = [channel.connect("on-message-string", receive), channel.connect("on-close", closed)]
        print(f"Browser input channel authorized for player {authorization.player}", file=sys.stderr, flush=True)

    def apply_offer_and_create_answer(
        self,
        offer_text: str,
        context: dict[str, object] | None = None,
    ) -> bool:
        try:
            if self.closed:
                raise RuntimeError("browser peer closed")
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
        if self.closed or self.webrtc is None:
            self.fail_pending_answer(
                context,
                RuntimeError("GStreamer WebRTC element is not initialized"),
            )
            return
        promise = Gst.Promise.new_with_change_func(self.on_answer_created, context)
        self.webrtc.emit("create-answer", None, promise)

    def on_answer_created(self, promise: Gst.Promise, context: dict[str, object] | None) -> None:
        if self.closed:
            self.fail_pending_answer(context, RuntimeError("browser peer closed"))
            return
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
        if self.closed:
            return False
        if self.webrtc is None:
            return True
        description = self.webrtc.get_property("local-description")
        if description is None:
            return True
        answer_text = normalize_sdp(description.sdp.as_text())
        try:
            validate_answer_candidates(answer_text, self.ice_ports)
        except RuntimeError as error:
            self.mark_pipeline_failed(str(error))
            return False
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
        if self.closed:
            return
        print(
            f"Raw-session WebRTC pipeline stopped session={self.args.session_id}: {reason}",
            file=sys.stderr,
            flush=True,
        )
        self.pipeline_failed = True
        if self.ice_timeout_id is not None:
            GLib.source_remove(self.ice_timeout_id)
            self.ice_timeout_id = None
        self.detach_callbacks()
        if self.input_authorization:
            self.input_relay.revoke(self.input_authorization.peer_id)
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


class MultiPeerSessionPublisher:
    """One frame-aligned FIFO reader per stream; bounded independent receivers."""
    read_fifo = RawSessionPublisher.read_fifo

    def __init__(self, args):
        self.args = args
        self.ice_ports = IceUdpPortRange.from_args(args)
        self.loop = GLib.MainLoop()
        self.reader_stop = threading.Event()
        self.pool = PeerPool(lambda relay: RawSessionPublisher(args, relay), args.max_peers)
        self.offer_lock = threading.Lock()

    def push_media_block(self, label, data):
        for _key, peer in self.pool.snapshot():
            try:
                peer.push_media_block(label, data)
            except Exception as error:
                GLib.idle_add(peer.mark_pipeline_failed, f"{label} peer feed: {error}")

    def mark_pipeline_failed(self, reason):
        for _key, peer in self.pool.snapshot():
            peer.mark_pipeline_failed(reason)

    def answer_for_offer(self, offer, host_ip, input_authorization=None, viewer_key=None):
        if not offer.lstrip().startswith("v=0") or "\nm=" not in offer or len(offer.encode("utf-8")) > 262144:
            raise RuntimeError("invalid SDP offer")
        with self.offer_lock:
            self.prune()
            key, peer = self.pool.create(input_authorization, viewer_key)
            try:
                answer = peer.answer_for_offer(offer, host_ip, input_authorization=input_authorization)
                print(f"Browser peer ready key={key} count={len(self.pool.snapshot())}", file=sys.stderr, flush=True)
                return answer
            except Exception:
                self.pool.remove(key, peer)
                raise

    def renew_input(self, authorization):
        return self.pool.renew(authorization)

    def prune(self):
        now = time.monotonic()
        for key, peer in self.pool.snapshot():
            if peer.expired(now):
                self.pool.remove(key, peer)

    def run_http_signaling(self):
        threads = []
        for label, path, size in (
            ("video", self.args.video_path, self.args.width * self.args.height * 4),
            ("audio", self.args.audio_path, 960 * 2 * 2),
        ):
            thread = threading.Thread(target=self.read_fifo, args=(label, path, size), daemon=True)
            thread.start()
            threads.append(thread)
        def maintenance():
            while not self.reader_stop.wait(1):
                self.prune()
        thread = threading.Thread(target=maintenance, daemon=True)
        thread.start()
        threads.append(thread)
        host, port = self.args.http_signal.rsplit(":", 1)
        server = ThreadingHTTPServer((host, int(port)), request_handler(self))
        threading.Thread(target=server.serve_forever, daemon=True).start()
        install_signal_handlers(self.loop)
        print(f"Raw session WebRTC publisher listening at http://{host}:{port}/sessions/{self.args.session_id}/offer max_peers={self.args.max_peers} ice_udp={self.ice_ports.start}-{self.ice_ports.end}", file=sys.stderr, flush=True)
        try:
            self.loop.run()
        finally:
            self.reader_stop.set()
            server.shutdown()
            server.server_close()
            for thread in threads:
                thread.join(timeout=1)
            self.pool.close()


def request_handler(publisher) -> type[BaseHTTPRequestHandler]:
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
            peers = f" peers={len(publisher.pool.snapshot())} max_peers={publisher.args.max_peers} ice_udp={publisher.ice_ports.start}-{publisher.ice_ports.end}" if isinstance(publisher, MultiPeerSessionPublisher) else ""
            self.wfile.write(
                f"4-Play raw session WebRTC publisher ready for {publisher.args.session_id}{peers}\n".encode(
                    "utf-8",
                )
            )

        def do_OPTIONS(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API.
            self.send_response(204)
            self.send_cors_headers()
            self.end_headers()

        def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API.
            if self.path == f"/sessions/{publisher.args.session_id}/input-lease":
                try:
                    authorization = InputAuthorization.from_headers(self.headers)
                    renewed = publisher.renew_input(authorization)
                except (ValueError, KeyError):
                    renewed = False
                self.send_response(204 if renewed else 409)
                self.end_headers()
                return
            valid_paths = {
                "/offer",
                f"/sessions/{publisher.args.session_id}/offer",
            }
            if self.path not in valid_paths:
                self.send_error(404, "not found")
                return
            try:
                length = int(self.headers.get("content-length", "0"))
                if not 1 <= length <= 262144:
                    self.send_error(413, "SDP offer must be between 1 and 262144 bytes")
                    return
                self.connection.settimeout(10)
                offer = self.rfile.read(length).decode("utf-8")
                print(
                    f"Received SDP offer path={self.path} bytes={len(offer)} "
                    f"client={self.client_address[0]}",
                    file=sys.stderr,
                    flush=True,
                )
                browser_host_ip = browser_host_for_request(self.headers, self.client_address[0], publisher.args.browser_host_ip)
                authorization = InputAuthorization.from_headers(self.headers)
                options = {"input_authorization": authorization}
                if isinstance(publisher, MultiPeerSessionPublisher):
                    options["viewer_key"] = self.headers.get("x-4play-viewer-key")
                answer = publisher.answer_for_offer(offer, browser_host_ip, **options)
            except PeerCapacityError:
                self.send_error(409, "session browser peer limit reached")
                return
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
    parser.add_argument("--max-peers", type=int, default=int(os.environ.get("FOURPLAY_WEBRTC_MAX_PEERS", "8")),
                        help="Bound browser receivers per session (1-32); default 8.")
    parser.add_argument(
        "--browser-host-ip",
        default=os.environ.get("FOURPLAY_WEBRTC_BROWSER_HOST_IP"),
        help="Fallback browser IP for manual signaling without trusted client address metadata.",
    )
    parser.add_argument("--verbose", action="store_true")
    add_ice_port_arguments(parser)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    Gst.init(None)
    MultiPeerSessionPublisher(args).run_http_signaling()


if __name__ == "__main__":
    main()
