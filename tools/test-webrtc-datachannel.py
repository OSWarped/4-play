"""Linux integration test: real WebRTC/SCTP channel -> authenticated UDP input.

No active game or control-plane service is used. Both peers run in this process.
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

from webrtc_browser_input import InputAuthorization


spec = importlib.util.spec_from_file_location("publisher", Path(__file__).with_name("webrtc-gst-raw-session-publisher.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
Gst, GLib, GstWebRTC, GstSdp = module.Gst, module.GLib, module.GstWebRTC, module.GstSdp
Gst.init(None)


def main():
    loop = GLib.MainLoop()
    loop_thread = threading.Thread(target=loop.run, daemon=True)
    loop_thread.start()
    receiver = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    receiver.bind(("127.0.0.1", 0))
    receiver.settimeout(10)
    client = Gst.Pipeline.new("input-client")
    peer = Gst.ElementFactory.make("webrtcbin", "client")
    peer.set_property("bundle-policy", GstWebRTC.WebRTCBundlePolicy.MAX_BUNDLE)
    peer.set_property("latency", 0)
    client.add(peer)
    publisher = None
    try:
        with tempfile.TemporaryDirectory(prefix="4play-input-check-") as directory:
            video = Path(directory, "video.raw")
            audio = Path(directory, "audio.pcm")
            video.touch(); audio.touch()
            publisher = module.RawSessionPublisher(argparse.Namespace(
                session_id="input-check", video_path=str(video), audio_path=str(audio),
                width=304, height=224, fps_num=60, fps_den=1, video_bitrate_kbps=1400,
                audio_bitrate_bps=64000, keyframe_distance=15, answer_file=None, verbose=False,
            ))
            authorization = InputAuthorization(uuid.uuid4().bytes, receiver.getsockname()[1],
                                               2, str(uuid.uuid4()), time.time_ns() // 1_000_000 + 60000)
            client.set_state(Gst.State.PLAYING)
            for caps in ["application/x-rtp,media=video,encoding-name=VP8,payload=96,clock-rate=90000",
                         "application/x-rtp,media=audio,encoding-name=OPUS,payload=111,clock-rate=48000"]:
                peer.emit("add-transceiver", GstWebRTC.WebRTCRTPTransceiverDirection.RECVONLY, Gst.Caps.from_string(caps))
            channel = peer.emit("create-data-channel", "4play-input", Gst.Structure.new_from_string(
                "application/data-channel,ordered=(boolean)false,max-retransmits=(int)0"))
            channel.connect("on-open", lambda channel: channel.emit("send-string", json.dumps(
                dict(sequence=1, buttons=3, axis_x=1, axis_y=-1))))
            offer_ready = threading.Event()
            context = {}

            def offer_created(promise, *_args):
                reply = promise.get_reply()
                generated = reply.get_value("offer")
                _, offer_sdp = GstSdp.SDPMessage.new_from_text(generated.sdp.as_text())
                offer = GstWebRTC.WebRTCSessionDescription.new(GstWebRTC.WebRTCSDPType.OFFER, offer_sdp)
                context["offer"] = offer
                offer_ready.set()

            peer.emit("create-offer", None, Gst.Promise.new_with_change_func(offer_created, None))
            assert offer_ready.wait(5), "client offer timeout"
            peer.emit("set-local-description", context["offer"], Gst.Promise.new())
            deadline = time.monotonic() + 5
            while peer.get_property("ice-gathering-state") != GstWebRTC.WebRTCICEGatheringState.COMPLETE:
                assert time.monotonic() < deadline, "client ICE gathering timeout"
                time.sleep(0.01)
            offer = peer.get_property("local-description").sdp.as_text()
            answer = publisher.answer_for_offer(offer, None, input_authorization=authorization)
            result, sdp = GstSdp.SDPMessage.new_from_text(answer)
            assert result == GstSdp.SDPResult.OK
            peer.emit("set-remote-description", GstWebRTC.WebRTCSessionDescription.new(
                GstWebRTC.WebRTCSDPType.ANSWER, sdp), Gst.Promise.new())
            data, _ = receiver.recvfrom(100)
            packet = struct.unpack("<4sB16sIHhhBB", data)
            assert packet == (b"4PLY", 2, authorization.token, 1, 3, 32767, -32767, 0, 2), packet
            channel.emit("send-string", json.dumps(dict(sequence=2, buttons=0, axis_x=0, axis_y=0)))
            neutral, _ = receiver.recvfrom(100)
            assert struct.unpack("<4sB16sIHhhBB", neutral)[4:] == (0, 0, 0, 0, 2)
            print("PASS: real WebRTC data channel forwarded a P2 diagonal/combo and release as authenticated UDP")
    finally:
        client.set_state(Gst.State.NULL)
        if publisher:
            publisher.pipeline.set_state(Gst.State.NULL)
            publisher.input_relay.close()
        receiver.close()
        loop.quit()
        loop_thread.join(timeout=2)


if __name__ == "__main__":
    main()
