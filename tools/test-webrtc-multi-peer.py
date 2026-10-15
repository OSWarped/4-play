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
import gc

from webrtc_browser_input import InputAuthorization
from webrtc_soak_metrics import interval_sample, progress_rates, read_process_sample


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
    def __init__(self, publisher, authorization, viewer_key=None, video_payload=96, audio_payload=111):
        self.pipeline = Gst.Pipeline.new(None)
        self.peer = Gst.ElementFactory.make("webrtcbin", None)
        self.peer.set_property("bundle-policy", GstWebRTC.WebRTCBundlePolicy.MAX_BUNDLE)
        self.peer.set_property("latency", 0)
        self.pipeline.add(self.peer)
        self.counts = dict(video=0, audio=0)
        self.channel = None
        self.channel_open = False
        self.channel_closed = False
        self.handlers = []
        self.handlers.append((self.peer, self.peer.connect("pad-added", self.pad_added)))
        self.pipeline.set_state(Gst.State.PLAYING)
        for caps in [f"application/x-rtp,media=video,encoding-name=VP8,payload={video_payload},clock-rate=90000",
                     f"application/x-rtp,media=audio,encoding-name=OPUS,payload={audio_payload},clock-rate=48000"]:
            self.peer.emit("add-transceiver", GstWebRTC.WebRTCRTPTransceiverDirection.RECVONLY, Gst.Caps.from_string(caps))
        # The spectator deliberately requests an input channel; the server must reject it.
        self.channel = self.peer.emit("create-data-channel", "4play-input", Gst.Structure.new_from_string(
            "application/data-channel,ordered=(boolean)false,max-retransmits=(int)0"))
        self.handlers.append((self.channel, self.channel.connect("on-open", lambda *_: setattr(self, "channel_open", True))))
        self.handlers.append((self.channel, self.channel.connect("on-close", lambda *_: setattr(self, "channel_closed", True))))
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
        sink = branch.get_by_name("sink")
        self.handlers.append((sink, sink.connect("handoff", lambda *_: self.counts.__setitem__(kind, self.counts[kind] + 1))))
        self.pipeline.add(branch)
        branch.sync_state_with_parent()
        assert pad.link(branch.get_static_pad("sink")) == Gst.PadLinkReturn.OK

    def send(self, sequence, buttons):
        self.channel.emit("send-string", json.dumps(dict(sequence=sequence, buttons=buttons, axis_x=1, axis_y=-1)))

    def close(self):
        self.pipeline.set_state(Gst.State.NULL)
        for owner, handler in self.handlers:
            owner.disconnect(handler)
        self.handlers.clear()


def soak(publisher, clients, authorizations, receive_combo, seconds, sample_seconds,
         reconnect_seconds, max_rss_mib):
    """Measure only this fixture: publisher AND three software decoding clients."""
    started = time.monotonic()
    deadline = started + seconds
    previous = read_process_sample()
    counts = [client.counts.copy() for client in clients]
    sequences = [10, 10, 10]
    reconnect_at = started + reconnect_seconds if reconnect_seconds else float('inf')
    samples, reconnects = [], 0
    print('SOAK ' + json.dumps(dict(event='start', seconds=seconds, peers=3,
          scope='combined generated publisher and three software receivers; not browser latency')),
          flush=True)
    while time.monotonic() < deadline:
        # Complete the final sample too: a sub-frame final interval cannot
        # reliably establish decoder progress and would report a false stall.
        time.sleep(sample_seconds)
        current = read_process_sample()
        measured = interval_sample(previous, current)
        measured['receivers'] = progress_rates(counts, [c.counts.copy() for c in clients],
            current['monotonic_seconds'] - previous['monotonic_seconds'])
        assert len(publisher.pool.snapshot()) == 3, 'peer count changed during soak'
        if max_rss_mib:
            assert current['rss_mib'] <= max_rss_mib, 'configured RSS ceiling exceeded'
        for index, authorization in enumerate(authorizations):
            renewed = replace(authorization, expires_ms=time.time_ns() // 1_000_000 + 60000)
            assert publisher.pool.renew(renewed), 'input lease renewal failed'
            authorizations[index] = renewed
            sequences[index] += 1
            clients[index].send(sequences[index], index + 1)
            receive_combo(renewed.player, index + 1)
        measured['elapsed_seconds'] = current['monotonic_seconds'] - started
        samples.append(measured)
        print('SOAK ' + json.dumps(dict(event='sample', **measured)), flush=True)
        if time.monotonic() >= reconnect_at and time.monotonic() < deadline:
            unaffected = [clients[i].counts['video'] for i in (0, 2)]
            clients[1].close()
            authorizations[1] = replace(authorizations[1], peer_id=str(uuid.uuid4()))
            clients[1] = Client(publisher, authorizations[1])
            wait_for(lambda: clients[1].channel_open and clients[1].counts['video'] > 10
                     and clients[1].counts['audio'] > 10, 'soak P2 reconnect')
            wait_for(lambda: all(clients[i].counts['video'] > count
                     for i, count in zip((0, 2), unaffected)), 'other peers continue')
            sequences[1] = 0
            reconnects += 1
            reconnect_at = time.monotonic() + reconnect_seconds
            gc.collect()
        # Reconnect resets the new receiver counters; exclude negotiation from
        # the next decode-rate interval rather than reporting negative rates.
        previous = read_process_sample()
        counts = [c.counts.copy() for c in clients]
    print('SOAK ' + json.dumps(dict(event='passed', samples=len(samples), reconnects=reconnects,
          elapsed_seconds=time.monotonic() - started,
          peak_rss_mib=max(s['rss_mib'] for s in samples),
          peak_cpu_percent=max(s['cpu_percent'] for s in samples),
          rss_change_mib=samples[-1]['rss_mib'] - samples[0]['rss_mib'],
          fd_change=samples[-1]['fds'] - samples[0]['fds'],
          peak_fds=max(s['fds'] for s in samples),
          peak_threads=max(s['threads'] for s in samples))), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--soak-seconds', type=int, default=0)
    parser.add_argument('--sample-seconds', type=int, default=5)
    parser.add_argument('--reconnect-seconds', type=int, default=30)
    parser.add_argument('--max-rss-mib', type=int, default=0,
                        help='optional combined fixture RSS ceiling; zero only observes')
    args = parser.parse_args()
    if not 0 <= args.soak_seconds <= 86400 or not 1 <= args.sample_seconds <= 60 or \
       not 0 <= args.reconnect_seconds <= 86400 or args.max_rss_mib < 0:
        parser.error('invalid soak duration, interval, or RSS ceiling')
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
                ice_udp_port_start=51000, ice_udp_port_count=100,
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
                clients.append(Client(publisher, auth, "spectator-grant" if auth is None else None,
                                      video_payload=100 if auth is auth1 else 96,
                                      audio_payload=109 if auth is auth1 else 111))
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
            old_ice = publisher.pool.peers["player:2"].webrtc.get_property("ice-agent")
            released = threading.Event()
            ice_watch = old_ice.weak_ref(lambda *_: released.set())
            del old_ice
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
            gc.collect()
            wait_for(released.is_set, "replaced P2 ICE native object released")
            if args.soak_seconds:
                active = [clients[0], clients[-1], clients[-2]]
                try:
                    soak(publisher, active, [auth1, auth2_new, auth3], receive_combo,
                         args.soak_seconds, args.sample_seconds, args.reconnect_seconds,
                         args.max_rss_mib)
                finally:
                    for client in active:
                        client.close()
                # Soak cleanup intentionally closes receivers; the short
                # malformed-offer checks below require the original live set.
                return
            # Failure in P2's own replacement must not reset P1 or the spectator.
            before = clients[0].counts["video"]
            try:
                publisher.answer_for_offer("not SDP", None, auth2_new)
                raise AssertionError("invalid offer unexpectedly succeeded")
            except RuntimeError:
                pass
            wait_for(lambda: clients[0].counts["video"] > before + 10, "P1 survives another peer's invalid offer")
            assert len(publisher.pool.snapshot()) == 3
            print("PASS: bounded UDP-only candidates; P1/P2/spectator audio/video and isolated input; promotion/reconnect; old ICE object released")
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
