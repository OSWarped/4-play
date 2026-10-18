"""WAN credentials/candidate/trust regression tests; no network or GI required."""
import base64
import hashlib
import hmac
import unittest
from unittest.mock import Mock, patch
from urllib.parse import unquote, urlsplit

from webrtc_ice_ports import IceUdpPortRange, validate_answer_candidates
from webrtc_wan import WanIce, request_is_wan


class WanTests(unittest.TestCase):
    def policy(self):
        return WanIce.from_environment({"FOURPLAY_WAN_TURN_HOST": "arcade.example.test",
            "FOURPLAY_WAN_TURN_SECRET": "a" * 64, "FOURPLAY_WAN_PUBLIC_IP": "45.24.46.155"})

    def test_lan_remains_default_and_profile_header_requires_loopback(self):
        self.assertFalse(request_is_wan({}, "192.168.20.10"))
        self.assertTrue(request_is_wan({"x-4play-media-profile": "wan"}, "127.0.0.1"))
        for headers, ip in [({"x-4play-media-profile": "wan"}, "192.168.20.10"),
                            ({"x-4play-media-profile": "unknown"}, "127.0.0.1")]:
            with self.assertRaises(ValueError): request_is_wan(headers, ip)

    def test_missing_or_unsafe_settings_fail_closed(self):
        with self.assertRaises(ValueError): WanIce.from_environment({})
        for key, value in [("FOURPLAY_WAN_TURN_HOST", "host/user"), ("FOURPLAY_WAN_TURN_SECRET", "short"),
                           ("FOURPLAY_WAN_PUBLIC_IP", "192.168.20.68"), ("FOURPLAY_WAN_PUBLIC_IP", "::1")]:
            settings = {"FOURPLAY_WAN_TURN_HOST": "arcade.test", "FOURPLAY_WAN_TURN_SECRET": "a"*64, "FOURPLAY_WAN_PUBLIC_IP": "45.24.46.155"}
            settings[key] = value
            with self.assertRaises(ValueError): WanIce.from_environment(settings)

    def test_gstreamer_credentials_expire_and_escape_userinfo_without_logging_key(self):
        policy = self.policy()
        self.assertNotIn("a"*64, repr(policy))
        peer = Mock()
        peer.emit.return_value = True
        with patch("webrtc_wan.uuid.uuid4", return_value="test-publisher"):
            policy.configure(peer, now=100)
        peer.set_property.assert_called_once_with("stun-server", "stun://arcade.example.test:3478")
        uri = urlsplit(peer.emit.call_args.args[1])
        username, password = unquote(uri.username), unquote(uri.password)
        self.assertEqual(username, "3700:publisher-test-publisher")
        self.assertEqual(password, base64.b64encode(hmac.new(b"a"*64, username.encode(), hashlib.sha1).digest()).decode())
        self.assertNotIn("a"*64, uri.geturl())
        peer.emit.return_value = False
        with self.assertRaisesRegex(RuntimeError, "could not configure"): policy.configure(peer)

    def test_candidate_types_and_each_port_pool_are_validated(self):
        answer = ("a=candidate:1 1 UDP 1 192.168.20.68 50001 typ host\r\n"
                  "a=candidate:2 1 UDP 1 45.24.46.155 60000 typ srflx raddr 192.168.20.68 rport 50001\r\n"
                  "a=candidate:3 1 UDP 1 45.24.46.155 52001 typ relay raddr 192.168.20.68 rport 50001\r\n")
        self.policy().validate_answer(answer, IceUdpPortRange())
        with self.assertRaises(RuntimeError): validate_answer_candidates(answer, IceUdpPortRange())
        for bad in [answer.replace("52001", "53000"), answer.replace("UDP", "TCP"),
                    answer.replace("50001", "41000"), answer.replace("45.24.46.155", "8.8.8.8"),
                    answer.replace("typ relay", "typ prflx"), answer.split("a=candidate:3")[0],
                    "a=candidate:broken", answer.replace("52001", "0"),
                    answer.replace("192.168.20.68", "127.0.0.1")]:
            with self.assertRaises(RuntimeError): self.policy().validate_answer(bad, IceUdpPortRange())


if __name__ == "__main__": unittest.main()
