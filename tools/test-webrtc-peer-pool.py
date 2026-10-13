"""Independent peer capacity, ownership, and source-lock regression tests."""
import json
import socket
import struct
import time
import unittest
import uuid
from dataclasses import replace

from webrtc_browser_input import InputAuthorization
from webrtc_peer_pool import PeerCapacityError, PeerPool


class FakePeer:
    def __init__(self, relay):
        self.relay = relay
        self.authorization = None
        self.closed = False

    def authorize(self, authorization):
        self.authorization = authorization
        if self.relay:
            self.relay.authorize(authorization)

    def close(self):
        self.closed = True
        if self.authorization:
            self.relay.revoke(self.authorization.peer_id)


class PoolTests(unittest.TestCase):
    def setUp(self):
        self.receiver = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.receiver.bind(("127.0.0.1", 0))
        self.receiver.settimeout(0.1)
        self.pool = PeerPool(FakePeer, 3)
        self.auth = InputAuthorization(uuid.uuid4().bytes, self.receiver.getsockname()[1],
                                       1, str(uuid.uuid4()), time.time_ns() // 1_000_000 + 60000)

    def tearDown(self):
        self.pool.close()
        self.receiver.close()

    def add(self, authorization=None):
        key, peer = self.pool.create(authorization)
        peer.authorize(authorization)
        return key, peer

    def test_two_players_and_spectator_have_independent_ownership(self):
        k1, p1 = self.add(self.auth)
        auth2 = replace(self.auth, player=2, peer_id=str(uuid.uuid4()))
        _, p2 = self.add(auth2)
        ks, viewer = self.add()
        self.assertIsNone(viewer.relay)
        self.pool.remove(ks, viewer)
        self.assertFalse(p1.closed)
        self.assertFalse(p2.closed)
        self.assertTrue(self.pool.renew(auth2))
        self.pool.remove(k1, p1)
        self.assertFalse(p2.closed)

    def test_capacity_rejection_preserves_existing_peers_and_own_reconnect(self):
        _, p1 = self.add(self.auth)
        _, p2 = self.add(replace(self.auth, player=2, peer_id=str(uuid.uuid4())))
        _, spectator = self.add()
        with self.assertRaises(PeerCapacityError):
            self.add()
        self.assertFalse(any(p.closed for p in [p1, p2, spectator]))
        _, replacement = self.add(replace(self.auth, peer_id=str(uuid.uuid4())))
        self.assertTrue(p1.closed)
        self.assertIs(replacement.relay, p1.relay)
        self.assertFalse(p2.closed)
        self.assertFalse(spectator.closed)

    def test_reconnect_keeps_udp_source_sequence_and_rejects_old_peer(self):
        key, old = self.add(self.auth)
        message = json.dumps(dict(sequence=1, buttons=3, axis_x=1, axis_y=-1))
        self.assertTrue(old.relay.receive(message, self.auth.peer_id))
        data, source = self.receiver.recvfrom(100)
        self.assertEqual(struct.unpack("<4sB16sIHhhBB", data)[3], 1)
        fresh = replace(self.auth, peer_id=str(uuid.uuid4()))
        _, new = self.add(fresh)
        self.receiver.recvfrom(100)  # Replaced controller neutralized.
        self.assertFalse(new.relay.receive(message, self.auth.peer_id))
        self.assertTrue(new.relay.receive(message, fresh.peer_id))
        data2, source2 = self.receiver.recvfrom(100)
        self.assertEqual(source, source2)
        self.assertEqual(struct.unpack("<4sB16sIHhhBB", data2)[3], 3)
        self.pool.remove(key, old)  # Late old cleanup cannot remove its successor.
        old.close()  # Late old revoke cannot clear successor's input authority.
        self.assertTrue(self.pool.renew(fresh))
        self.assertEqual(len(self.pool.snapshot()), 1)

    def test_spectator_reconnect_replaces_only_same_seat_grant(self):
        _, player = self.add(self.auth)
        key, viewer = self.pool.create(None, "seat-two:grant-a")
        _, other = self.pool.create(None, "seat-three:grant-b")
        new_key, replacement = self.pool.create(None, "seat-two:grant-a")
        self.assertEqual(key, new_key)
        self.assertTrue(viewer.closed)
        self.assertFalse(other.closed)
        self.assertFalse(player.closed)
        self.assertIsNone(replacement.relay)


if __name__ == "__main__":
    unittest.main()
