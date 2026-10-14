"""Linux integration: simultaneous decoded media, isolated SCTP input, reconnect.

Runs only throwaway local peers; no game, service, ROM, or production port is used.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import socket
import struct
import tempfile
import threading
import time
import uuid
from dataclasses import replace

from webrtc_browser_input import InputAuthorization


spec = importlib.util.spec_from_file_location("raw_publisher", Path(__file__).with_name("webrtc-gst-raw-session-publisher.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
Gst, GLib, GstSdp, GstWebRTC = module.Gst, module.GLib, module.GstSdp, module.GstWebRTC
Gst.init(None)


def wait_for(predicate, label, timeout=10):
    deadline = time.monotonic() + timeout
    while not predicate():
        if time.monotonic() >= deadline:
            raise AssertionError(f"timed out: {label}")
        time.sleep(0.02)


class Client:
    def __init__(self, publisher, authorization, viewer_key=None):
        self.pipeline = Gst.Pipeline.new(None)
        self.peer = Gst.ElementFactory.make("webrtcbin", None)
        self.peer.set_property("bundle-policy", GstWebRTC.WebRTCBundlePolicy.MAX_BUNDLE)
        self.peer.set_property("latency", 0)
        self.pipeline.add(self.peer)
        self.counts = dict(video=0, audio=0)
        self.channel = None
        self.channel_open = False
        self.channel_closed = False
        self.peer.connect("pad-added", self.pad_added)
        self.pipeline.set_state(Gst.State.PLAYING)
        for caps in ["application/x-rtp,media=video,encoding-name=VP8,payload=96,clock-rate=90000",
                     "application/x-rtp,media=audio,encoding-name=OPUS,payload=111,clock-rate=48000"]:
            self.peer.emit("add-transceiver", GstWebRTC.WebRTCRTPTransceiverDirection.RECVONLY, Gst.Caps.from_string(caps))
        # The spectator deliberately requests an input channel; the server must reject it.
        self.channel = self.peer.emit("create-data-channel", "4play-input", Gst.Structure.new_from_string(
            "application/data-channel,ordered=(boolean)false,max-retransmits=(int)0"))
        self.channel.connect("on-open", lambda *_: setattr(self, "channel_open", True))
        self.channel.connect("on-close", lambda *_: setattr(self, "channel_closed", True))
        event = threading.Event()
        context = {}

        def offered(promise, *_):
            reply = promise.get_reply()
            generated = reply.get_value("offer")
            _, sdp = GstSdp.SDPMessage.new_from_text(generated.sdp.as_text())
            context["offer"] = GstWebRTC.WebRTCSessionDescription.new(GstWebRTC.WebRTCSDPType.OFFER, sdp)
            event.set()

        self.peer.emit("create-offer", None, Gst.Promise.new_with_change_func(offered, None))
        assert event.wait(5), "offer timeout"
        self.peer.emit("set-local-description", context["offer"], Gst.Promise.new())
        wait_for(lambda: self.peer.get_property("ice-gathering-state") == GstWebRTC.WebRTCICEGatheringState.COMPLETE, "client ICE gathering")
        answer = publisher.answer_for_offer(self.peer.get_property("local-description").sdp.as_text(), None, authorization, viewer_key)
        result, sdp = GstSdp.SDPMessage.new_from_text(answer)
        assert result == GstSdp.SDPResult.OK
        self.peer.emit("set-remote-description", GstWebRTC.WebRTCSessionDescription.new(
            GstWebRTC.WebRTCSDPType.ANSWER, sdp), Gst.Promise.new())

    def pad_added(self, _peer, pad):
        caps = pad.get_current_caps() or pad.query_caps(None)
        kind = "video" if caps.can_intersect(Gst.Caps.from_string("application/x-rtp,media=video")) else "audio"
        if kind not in self.counts:
            return
        decoder = "rtpvp8depay ! vp8dec" if kind == "video" else "rtpopusdepay ! opusdec"
        branch = Gst.parse_bin_from_description(f"queue ! {decoder} ! fakesink name=sink sync=false signal-handoffs=true", True)
        branch.get_by_name("sink").connect("handoff", lambda *_: self.counts.__setitem__(kind, self.counts[kind] + 1))
        self.pipeline.add(branch)
        branch.sync_state_with_parent()
        assert pad.link(branch.get_static_pad("sink")) == Gst.PadLinkReturn.OK

    def send(self, sequence, buttons):
        self.channel.emit("send-string", json.dumps(dict(sequence=sequence, buttons=buttons, axis_x=1, axis_y=-1)))

    def close(self):
        self.pipeline.set_state(Gst.State.NULL)


def main():
    loop = GLib.MainLoop()
    loop_thread = threading.Thread(target=loop.run, daemon=True)
    loop_thread.start()
    receiver = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    receiver.bind(("127.0.0.1", 0))
    receiver.settimeout(5)
    clients = []
    publisher = None
    stop = threading.Event()
    feeder = None
    try:
        with tempfile.TemporaryDirectory(prefix="4play-multi-peer-check-") as directory:
            video, audio = Path(directory, "video.raw"), Path(directory, "audio.pcm")
            video.touch(); audio.touch()
            publisher = module.MultiPeerSessionPublisher(argparse.Namespace(
                session_id="multi-peer-check", video_path=str(video), audio_path=str(audio),
                width=304, height=224, fps_num=60, fps_den=1, video_bitrate_kbps=1400,
                audio_bitrate_bps=64000, keyframe_distance=15, answer_file=None, verbose=False, max_peers=3,
            ))

            def feed():
                frame = 0
                next_frame = time.monotonic()
                while not stop.wait(max(0, next_frame - time.monotonic())):
                    publisher.push_media_block("video", bytearray([frame % 256, 0, 0, 0]) * (304 * 224))
                    if frame % 6 != 1:
                        publisher.push_media_block("audio", bytearray(3840))
                    frame += 1
                    next_frame += 1 / 60

            feeder = threading.Thread(target=feed, daemon=True)
            feeder.start()
            auth1 = InputAuthorization(uuid.uuid4().bytes, receiver.getsockname()[1], 1,
                                       str(uuid.uuid4()), time.time_ns() // 1_000_000 + 60000)
            auth2 = replace(auth1, player=2, peer_id=str(uuid.uuid4()))
            for auth in [auth1, auth2, None]:
                clients.append(Client(publisher, auth, "spectator-grant" if auth is None else None))
            wait_for(lambda: all(c.counts["video"] > 10 and c.counts["audio"] > 10 for c in clients), "all three peers decode video/audio")
            wait_for(lambda: clients[0].channel_open and clients[1].channel_open, "player input channels")
            wait_for(lambda: clients[2].channel_closed, "spectator input rejected")
            sources = {}

            def receive_combo(player, buttons):
                while True:
                    data, source = receiver.recvfrom(100)
                    packet = struct.unpack("<4sB16sIHhhBB", data)
                    if packet[-1] == player and packet[4] == buttons:
                        assert packet[2] == auth1.token
                        assert packet[5:8] == (32767, -32767, 0)
                        return source, packet[3]

            clients[0].send(1, 3); source1, sequence1 = receive_combo(1, 3)
            clients[1].send(1, 12); source2, sequence2 = receive_combo(2, 12)
            assert source1 != source2, "players must have distinct UDP sources"
            before = clients[0].counts.copy()
            clients[2].close()
            clients.append(Client(publisher, None, "spectator-grant"))
            wait_for(lambda: clients[-1].counts["video"] > 10, "spectator reconnect at capacity")
            assert len(publisher.pool.snapshot()) == 3
            auth3 = replace(auth1, player=3, peer_id=str(uuid.uuid4()))
            clients.append(Client(publisher, auth3, "spectator-grant"))
            wait_for(lambda: clients[-1].channel_open and clients[-1].counts["video"] > 10, "spectator promotes to P3 at capacity")
            assert len(publisher.pool.snapshot()) == 3
            clients[-1].send(1, 5)
            source3, _ = receive_combo(3, 5)
            assert source3 not in (source1, source2)
            wait_for(lambda: clients[0].counts["video"] > before["video"] + 10, "P1 continues after spectator closes")
            clients[1].close()
            auth2_new = replace(auth2, peer_id=str(uuid.uuid4()))
            clients.append(Client(publisher, auth2_new))
            wait_for(lambda: clients[-1].channel_open and clients[-1].counts["video"] > 10, "P2 reconnect")
            clients[-1].send(1, 48)
            new_source, new_sequence = receive_combo(2, 48)
            assert new_source == source2 and new_sequence > sequence2
            clients[0].send(2, 7)
            continued_source, continued_sequence = receive_combo(1, 7)
            assert continued_source == source1 and continued_sequence > sequence1
            assert publisher.pool.renew(auth1) and publisher.pool.renew(auth2_new)
            assert not publisher.pool.renew(auth2)
            # Failure in P2's own replacement must not reset P1 or the spectator.
            before = clients[0].counts["video"]
            try:
                publisher.answer_for_offer("not SDP", None, auth2_new)
                raise AssertionError("invalid offer unexpectedly succeeded")
            except RuntimeError:
                pass
            wait_for(lambda: clients[0].counts["video"] > before + 10, "P1 survives another peer's invalid offer")
            assert len(publisher.pool.snapshot()) == 3
            print("PASS: P1/P2/spectator decode audio/video; spectator input rejected; promotion at capacity and P2 reconnect preserve other players")
    finally:
        stop.set()
        if feeder:
            feeder.join(timeout=2)
        for client in clients:
            client.close()
        if publisher:
            publisher.pool.close()
        receiver.close()
        loop.quit()
        loop_thread.join(timeout=2)


if __name__ == "__main__":
    main()
