"""Real pattern playback and exhausted-port fail-closed checks; Linux GI needed."""
import argparse
import importlib.util
from pathlib import Path
import socket
import tempfile
import threading

from webrtc_ice_ports import IceUdpPortRange, validate_answer_candidates


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


integration = load("multi_peer_checks", "test-webrtc-multi-peer.py")
pattern_module = load("test_pattern", "webrtc-gst-test-pattern.py")
Gst, GLib = integration.Gst, integration.GLib


def block_port():
    for _ in range(100):
        sockets = []
        try:
            ipv4 = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            sockets.append(ipv4)
            ipv4.bind(("0.0.0.0", 0))
            port = ipv4.getsockname()[1]
            IceUdpPortRange(port, 1).validate_service_pools({})
            if socket.has_ipv6:
                ipv6 = socket.socket(socket.AF_INET6, socket.SOCK_DGRAM)
                sockets.append(ipv6)
                ipv6.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                ipv6.bind(("::", port))
            return port, sockets
        except (OSError, ValueError):
            for held in sockets:
                held.close()
    raise AssertionError("could not reserve a port on all IP families for exhaustion test")


def main():
    loop = GLib.MainLoop()
    thread = threading.Thread(target=loop.run, daemon=True)
    thread.start()
    pattern = client = publisher = None
    held = []
    try:
        args = argparse.Namespace(codec="vp8", pattern="ball", width=320, height=224,
            fps=60, video_only=False, video_bitrate_kbps=1400, audio_bitrate_bps=64000,
            audio_payload_type=111, verbose=False, answer_file=None,
            ice_udp_port_start=52000, ice_udp_port_count=100)
        pattern = pattern_module.WebRtcTestPatternPublisher(args)

        class Adapter:
            def answer_for_offer(self, offer, host, _authorization=None, _viewer=None):
                answer = pattern.answer_for_offer(offer, host)
                validate_answer_candidates(answer, IceUdpPortRange(52000, 100))
                return answer

        client = integration.Client(Adapter(), None)
        integration.wait_for(lambda: client.counts["video"] > 10 and client.counts["audio"] > 10,
                             "pattern decodes audio/video over bounded UDP")
        port, held = block_port()
        with tempfile.TemporaryDirectory(prefix="4play-ice-exhaustion-") as directory:
            video, audio = Path(directory, "video.raw"), Path(directory, "audio.pcm")
            video.touch(); audio.touch()
            publisher = integration.module.MultiPeerSessionPublisher(argparse.Namespace(
                session_id="exhaustion-check", video_path=str(video), audio_path=str(audio),
                width=320, height=224, fps_num=60, fps_den=1, video_bitrate_kbps=1400,
                audio_bitrate_bps=64000, keyframe_distance=15, answer_file=None, verbose=False,
                max_peers=1, ice_udp_port_start=port, ice_udp_port_count=1))
            offer = client.peer.get_property("local-description").sdp.as_text()
            try:
                publisher.answer_for_offer(offer, None)
                raise AssertionError("exhausted ICE range unexpectedly returned an answer")
            except RuntimeError as error:
                assert "candidate" in str(error).lower() or "timed out" in str(error).lower(), error
            assert not publisher.pool.snapshot(), "failed peer must be removed"
        print("PASS: standalone pattern decodes bounded UDP audio/video; occupied range fails without unbounded fallback")
    finally:
        if client:
            client.close()
        if publisher:
            publisher.pool.close()
        if pattern:
            if pattern.ice_timeout_id is not None:
                GLib.source_remove(pattern.ice_timeout_id)
            pattern.pipeline.get_bus().remove_signal_watch()
            pattern.pipeline.set_state(Gst.State.NULL)
        for item in held:
            item.close()
        loop.quit()
        thread.join(timeout=2)


if __name__ == "__main__":
    main()
