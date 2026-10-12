"""Check browser relay authorization and actual authenticated UDP packets."""
import json
import socket
import struct
import time
import unittest
import uuid
from dataclasses import replace
from webrtc_browser_input import BrowserInputRelay, InputAuthorization


class RelayTests(unittest.TestCase):
    def setUp(self):
        self.receiver = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.receiver.bind(("127.0.0.1", 0))
        self.receiver.settimeout(0.1)
        self.relay = BrowserInputRelay()
        self.auth = InputAuthorization(uuid.uuid4().bytes, self.receiver.getsockname()[1],
                                       2, str(uuid.uuid4()), time.time_ns() // 1_000_000 + 60000)
        self.relay.authorize(self.auth)

    def tearDown(self):
        self.relay.close()
        self.receiver.close()

    def message(self, sequence=1, **overrides):
        return json.dumps(dict(sequence=sequence, buttons=3, axis_x=1, axis_y=-1, **overrides))

    def packet(self):
        data, source = self.receiver.recvfrom(100)
        self.assertEqual(len(data), 33)
        return struct.unpack("<4sB16sIHhhBB", data), source

    def test_combo_diagonal_neutral_and_persistent_udp_source(self):
        self.assertTrue(self.relay.receive(self.message(), self.auth.peer_id))
        packet, source = self.packet()
        self.assertEqual(packet, (b"4PLY", 2, self.auth.token, 1, 3, 32767, -32767, 0, 2))
        self.relay.neutralize(self.auth.peer_id)
        neutral, source2 = self.packet()
        self.assertEqual(neutral[4:], (0, 0, 0, 0, 2))
        self.assertEqual(source, source2)
        self.assertFalse(self.relay.receive(self.message(), self.auth.peer_id))
        self.assertTrue(self.relay.receive(self.message(2), self.auth.peer_id))
        self.assertEqual(self.packet()[0][3], 3)

    def test_spectator_and_stale_peer_cannot_send_input(self):
        self.assertFalse(self.relay.receive(self.message(), "other-peer"))
        self.relay.authorize(None)
        self.packet()  # Old player neutralized.
        self.assertFalse(self.relay.receive(self.message(), self.auth.peer_id))
        with self.assertRaises(socket.timeout):
            self.receiver.recvfrom(100)

    def test_lease_renewal_checks_peer_and_expiry_neutralizes(self):
        self.assertFalse(self.relay.renew(replace(self.auth, peer_id=str(uuid.uuid4()))))
        self.assertTrue(self.relay.renew(replace(self.auth, expires_ms=0)))
        self.assertFalse(self.relay.receive(self.message(), self.auth.peer_id))
        self.assertEqual(self.packet()[0][4], 0)
        self.assertIsNone(self.relay.authorization)

    def test_invalid_states_cannot_set_player_or_stop_flags(self):
        for value in ["bad json", "[]", "{}", "x" * 513,
                      '{"sequence":1,"buttons":true,"axis_x":0,"axis_y":0}',
                      '{"sequence":1,"buttons":256,"axis_x":0,"axis_y":0}']:
            self.assertFalse(self.relay.receive(value, self.auth.peer_id))
        message = json.loads(self.message())
        message.update(player_slot=1, flags=1)
        self.assertTrue(self.relay.receive(json.dumps(message), self.auth.peer_id))
        packet, _ = self.packet()
        self.assertEqual(packet[-2:], (0, 2))

    def test_reconnect_rejects_old_peer_and_accepts_fresh_sequence(self):
        self.relay.receive(self.message(400), self.auth.peer_id)
        self.packet()
        new = replace(self.auth, peer_id=str(uuid.uuid4()))
        self.relay.authorize(new)
        self.packet()
        self.assertFalse(self.relay.receive(self.message(401), self.auth.peer_id))
        self.assertTrue(self.relay.receive(self.message(1), new.peer_id))
        self.assertEqual(self.packet()[0][3], 3)


if __name__ == "__main__":
    unittest.main()
