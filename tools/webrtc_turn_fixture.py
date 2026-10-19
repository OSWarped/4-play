"""Test-only forced-relay media fixture; never import in production publishers.

Loopback mode substitutes the TURN endpoint and candidate-address/range policy.
Private-profile mode uses the real WAN validator without address rewriting and
a simulated public interface inside a disposable namespace. The real sender,
encoders, SCTP input relay and REST credential generation execute unchanged.
"""
import argparse
import importlib.util
from pathlib import Path
import socket
import sqlite3
from contextlib import closing
import struct
import threading
import time
import uuid
from unittest.mock import patch
from dataclasses import replace

from webrtc_wan import WanIce
from webrtc_browser_input import InputAuthorization
from webrtc_wan_admission import WanAdmission
from turn_socket_observation import owned_udp_sockets


def relay_sockets(pid, start=62000, end=62031):
    """Observe only this fixture's coturn-owned sockets, not unrelated services."""
    return set(owned_udp_sockets(pid, start, end))


def test_relay_media(port, secret, root, relay_pid, *, private_profile=False, reconnect=False):
    spec = importlib.util.spec_from_file_location("relay_shared_fixture", Path(__file__).with_name("test-webrtc-multi-peer.py"))
    shared = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(shared)
    raw = shared.module
    Gst, GLib, GstWebRTC = raw.Gst, raw.GLib, raw.GstWebRTC

    class Endpoint:
        def __init__(self, peer): self.peer = peer
        def set_property(self, name, uri):
            self.peer.set_property(name, uri.replace("localhost:3478", f"127.0.0.1:{port}"))
        def emit(self, name, uri):
            return self.peer.emit(name, uri.replace("localhost:3478", f"127.0.0.1:{port}"))

    class LoopbackPolicy(WanIce):
        def configure(self, peer, now=None):
            super().configure(Endpoint(peer), now)
            peer.set_property("ice-transport-policy", GstWebRTC.WebRTCICETransportPolicy.RELAY)
            assert peer.get_property("ice-transport-policy") == GstWebRTC.WebRTCICETransportPolicy.RELAY

        def validate_answer(self, answer, ports):
            # Test candidates really use loopback 62000..62031. Map only the copy
            # passed into the strict production validator to its declared public
            # fixture address/range; never alter addresses in the negotiated SDP.
            mapped = []
            relay_found = False
            for line in answer.splitlines():
                if line.startswith("a=candidate:"):
                    fields = line.split()
                    if fields[7] == "relay":
                        assert fields[4] == "127.0.0.1" and 62000 <= int(fields[5]) <= 62031
                        fields[4] = self.public_ip
                        fields[5] = str(52000 + int(fields[5]) - 62000)
                        relay_found = True
                    elif fields[7] == "srflx" and fields[4] == "127.0.0.1":
                        fields[4] = self.public_ip
                    line = " ".join(fields)
                mapped.append(line)
            assert relay_found, "No real loopback TURN relay candidate"
            super().validate_answer("\r\n".join(mapped), ports)

    class PrivateProfilePolicy(WanIce):
        def configure(self, peer, now=None):
            super().configure(peer, now)
            peer.set_property("ice-transport-policy", GstWebRTC.WebRTCICETransportPolicy.RELAY)
            assert peer.get_property("ice-transport-policy") == GstWebRTC.WebRTCICETransportPolicy.RELAY
        # Unlike the loopback adapter, inherited candidate validation is used
        # unchanged: real public fixture address and production relay ports.

    policy = (PrivateProfilePolicy("192.168.20.68", secret, "45.24.46.155") if private_profile
              else LoopbackPolicy("localhost", secret, "45.24.46.155"))
    def configure_client(peer):
        if private_profile:
            # Exercise UDP relay media, not libnice's additional speculative
            # TCP allocations. TCP TURN protocol is a separate service check.
            raw.configure_ice_udp_ports(peer, raw.IceUdpPortRange(61000, 100))
        policy.configure(peer)
    relay_start, relay_end = (52000, 52199) if private_profile else (62000, 62031)
    def allocated_sockets(): return relay_sockets(relay_pid, relay_start, relay_end)
    loop = GLib.MainLoop()
    loop_thread = threading.Thread(target=loop.run, daemon=True)
    loop_thread.start()
    stop = threading.Event()
    receiver = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    receiver.bind(("127.0.0.1", 0))
    receiver.settimeout(5)
    clients = []
    publisher = None
    feeder = None
    maintenance = None
    shared.wait_for(lambda: not allocated_sockets(), "protocol relay sockets released", timeout=5)
    try:
        video, audio = root / "video.raw", root / "audio.pcm"
        video.touch(); audio.touch()
        publisher = raw.MultiPeerSessionPublisher(argparse.Namespace(
            session_id="forced-relay-check", video_path=str(video), audio_path=str(audio),
            width=304, height=224, fps_num=60, fps_den=1, video_bitrate_kbps=1400,
            audio_bitrate_bps=64000, keyframe_distance=15, answer_file=None, verbose=False,
            max_peers=3, ice_udp_port_start=50000 if private_profile else 61000,
            ice_udp_port_count=1000 if private_profile else 100))
        database = root / "test-admission.db"
        database.touch(mode=0o600)
        player_hash, viewer_hash = "a"*64, "b"*64
        now_ms = time.time_ns() // 1_000_000
        with closing(sqlite3.connect(database)) as db:
            db.executescript("""CREATE TABLE tester_invitations(id TEXT PRIMARY KEY, created_ms INTEGER, expires_ms INTEGER, revoked INTEGER);
                CREATE TABLE tester_admissions(credential_hash TEXT PRIMARY KEY, invitation_id TEXT, expires_ms INTEGER);""")
            for name, fingerprint in [("player",player_hash),("viewer",viewer_hash)]:
                db.execute("INSERT INTO tester_invitations VALUES (?,?,?,0)", (name,now_ms,now_ms+120_000))
                db.execute("INSERT INTO tester_admissions VALUES (?,?,?)", (fingerprint,name,now_ms+120_000))
            db.commit()
        player_guard, viewer_guard = WanAdmission(database, player_hash), WanAdmission(database, viewer_hash)

        def maintain():
            while not stop.wait(.1): publisher.prune()
        maintenance = threading.Thread(target=maintain, daemon=True)
        maintenance.start()

        def feed():
            frame = 0
            next_frame = time.monotonic()
            while not stop.wait(max(0, next_frame - time.monotonic())):
                publisher.push_media_block("video", bytearray([frame % 256, 0, 0, 0]) * (304 * 224))
                if frame % 6 != 1: publisher.push_media_block("audio", bytearray(3840))
                frame += 1
                next_frame += 1/60

        feeder = threading.Thread(target=feed, daemon=True)
        feeder.start()
        auth = InputAuthorization(uuid.uuid4().bytes, receiver.getsockname()[1], 1, str(uuid.uuid4()), time.time_ns() // 1_000_000 + 120_000)
        with patch.object(raw.WanIce, "from_environment", return_value=policy):
            def signal(offer, host, authorization, viewer_key):
                return publisher.answer_for_offer(offer, host, authorization, viewer_key, wan_media=True, wan_admission=player_guard)
            client = shared.Client(publisher, auth, configure_peer=configure_client, signal=signal)
        clients.append(client)
        shared.wait_for(lambda: client.counts["video"] > 15 and client.counts["audio"] > 15 and client.channel_open,
                        "forced relay decodes audio/video and opens input", timeout=15)
        server_peer = publisher.pool.peers["player:1"]
        assert server_peer.wan_mode and server_peer.wan_ice is policy
        assert server_peer.webrtc.get_property("ice-transport-policy") == GstWebRTC.WebRTCICETransportPolicy.RELAY
        assert len(allocated_sockets()) >= 2, "No coturn-owned relay sockets observed"

        def receive_buttons(expected):
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                data, _ = receiver.recvfrom(100)
                packet = struct.unpack("<4sB16sIHhhBB", data)
                if packet[4] == expected:
                    assert packet[2] == auth.token and packet[-1] == 1
                    return
            raise AssertionError("Relayed SCTP controller state missing")

        client.send(1, 3)
        receive_buttons(3)
        client.send(2, 0)
        receive_buttons(0)
        print("PASS: forced relay on both peers; decoded video/audio and SCTP press/release")

        # A LAN spectator must retain host-only ICE, not inherit WAN settings.
        spectator = shared.Client(publisher, None, "local-spectator")
        clients.append(spectator)
        shared.wait_for(lambda: spectator.counts["video"] > 10 and spectator.counts["audio"] > 10,
                        "LAN spectator continues beside relayed player")
        local_peer = publisher.pool.peers["viewer:local-spectator"]
        assert not local_peer.wan_mode and local_peer.wan_ice is None
        before = client.counts["video"]
        shared.wait_for(lambda: client.counts["video"] > before + 15, "relay continues beside LAN spectator")
        print("PASS: simultaneous LAN peer remains host-only; relay media continues")
        if reconnect:
            old_peer, old_client = server_peer, client
            old_allocations = allocated_sockets()
            old_client.close()
            auth = replace(auth, peer_id=str(uuid.uuid4()))
            with patch.object(raw.WanIce, "from_environment", return_value=policy):
                client = shared.Client(publisher, auth, configure_peer=configure_client, signal=signal)
            clients.append(client)
            shared.wait_for(lambda: client.counts["video"] > 15 and client.counts["audio"] > 15 and client.channel_open,
                            "reconnected player decodes media and opens input", timeout=15)
            server_peer = publisher.pool.peers["player:1"]
            assert server_peer is not old_peer and old_peer.closed
            shared.wait_for(lambda: old_peer.turn_cleanup.done.is_set() and old_client.turn_cleanup.done.is_set(),
                            "old reconnect allocations cleaned")
            assert old_peer.turn_cleanup.released and old_client.turn_cleanup.released
            shared.wait_for(lambda: not old_allocations.intersection(allocated_sockets()),
                            "coturn releases replaced player sockets", timeout=5)
            client.send(1, 1)
            receive_buttons(1)
            client.send(2, 0)
            receive_buttons(0)
            before = spectator.counts["video"]
            shared.wait_for(lambda: spectator.counts["video"] > before + 10, "LAN survives player reconnect")
            print("PASS: player leave/rejoin resumes media/input, releases old allocations and preserves LAN viewing")
        player_allocations = allocated_sockets()
        with patch.object(raw.WanIce, "from_environment", return_value=policy):
            def viewer_signal(offer, host, authorization, viewer_key):
                return publisher.answer_for_offer(offer, host, authorization, viewer_key, wan_media=True, wan_admission=viewer_guard)
            viewer = shared.Client(publisher, None, "wan-spectator", configure_peer=configure_client, signal=viewer_signal)
        clients.append(viewer)
        shared.wait_for(lambda: viewer.counts["video"] > 10 and viewer.counts["audio"] > 10,
                        "relayed spectator decodes media")
        viewer_peer = publisher.pool.peers["viewer:wan-spectator"]
        if reconnect:
            # Preserve the player's allocations; observe only the departing
            # spectator's native sockets by subtracting the earlier baseline.
            viewer_allocations = allocated_sockets() - player_allocations
            assert viewer_allocations, "No spectator relay sockets observed"
            viewer.close()
            publisher.pool.remove("viewer:wan-spectator", viewer_peer)
            shared.wait_for(lambda: viewer_peer.turn_cleanup.done.is_set() and viewer.turn_cleanup.done.is_set(),
                            "spectator leave allocations cleaned")
            assert viewer_peer.turn_cleanup.released and viewer.turn_cleanup.released
            shared.wait_for(lambda: not viewer_allocations.intersection(allocated_sockets()),
                            "coturn releases departed spectator sockets", timeout=5)
            before = client.counts["video"]
            shared.wait_for(lambda: client.counts["video"] > before + 10, "player survives spectator leave")
            with patch.object(raw.WanIce, "from_environment", return_value=policy):
                viewer = shared.Client(publisher, None, "wan-spectator", configure_peer=configure_client, signal=viewer_signal)
            clients.append(viewer)
            shared.wait_for(lambda: viewer.counts["video"] > 10 and viewer.counts["audio"] > 10,
                            "spectator rejoins with decoded media", timeout=15)
            viewer_peer = publisher.pool.peers["viewer:wan-spectator"]
            print("PASS: spectator leave/rejoin releases its allocations without interrupting the player")
        with closing(sqlite3.connect(database)) as db:
            db.execute("UPDATE tester_invitations SET revoked=1 WHERE id='player'")
            db.commit()
        shared.wait_for(lambda: "player:1" not in publisher.pool.peers, "revoked player peer closed", timeout=4)
        assert server_peer.closed
        shared.wait_for(lambda: server_peer.turn_cleanup.done.is_set(), "revoked player's TURN release")
        assert server_peer.turn_cleanup.released
        before = viewer.counts["video"]
        shared.wait_for(lambda: viewer.counts["video"] > before + 10, "unrevoked WAN viewer survives")
        with closing(sqlite3.connect(database)) as db:
            db.execute("UPDATE tester_admissions SET expires_ms=0 WHERE invitation_id='viewer'")
            db.commit()
        shared.wait_for(lambda: "viewer:wan-spectator" not in publisher.pool.peers, "expired spectator peer closed", timeout=4)
        shared.wait_for(lambda: viewer_peer.turn_cleanup.done.is_set(), "expired spectator's TURN release")
        assert viewer_peer.turn_cleanup.released
        before = spectator.counts["video"]
        shared.wait_for(lambda: spectator.counts["video"] > before + 10, "LAN viewer survives WAN revoke/expiry")
        print("PASS: player revocation and spectator expiry close only their WAN peers; other viewers continue")
    finally:
        stop.set()
        if feeder: feeder.join(timeout=2)
        if maintenance: maintenance.join(timeout=2)
        for client in clients: client.close()
        if publisher: publisher.pool.close()
        receiver.close()
        assert raw.drain_turn_cleanup(GLib), "TURN cleanup tasks did not complete"
        loop.quit()
        loop_thread.join(timeout=2)
        shared.wait_for(lambda: not allocated_sockets(), "all coturn relay sockets released before server shutdown")
        print("PASS: async cleanup released coturn-owned relay ports while relay server remained running")
