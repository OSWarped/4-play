"""Port policy and read-only firewall planner tests, without GI bindings."""
import argparse
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from webrtc_ice_ports import IceUdpPortRange, configure_ice_udp_ports, validate_answer_candidates, add_ice_port_arguments

spec = importlib.util.spec_from_file_location("firewall_plan", Path(__file__).with_name("webrtc-firewall-plan.py"))
planner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(planner)


class Nice:
    def __init__(self): self.upnp = True
    def find_property(self, name): return name
    def set_property(self, _name, value): self.upnp = value
    def get_property(self, _name): return self.upnp


class PortTests(unittest.TestCase):
    def test_defaults_bounds_and_overlap_rejection(self):
        self.assertEqual(IceUdpPortRange().end, 50999)
        self.assertEqual(IceUdpPortRange(65535, 1).end, 65535)
        for start, count in [(0, 100), (50000, 0), (-1, 2), (65535, 2), (True, 100)]:
            with self.assertRaises(ValueError):
                IceUdpPortRange(start, count)
        with self.assertRaisesRegex(ValueError, "input port pool"):
            IceUdpPortRange(42000, 10).validate_service_pools({})
        with self.assertRaisesRegex(ValueError, "media port pool"):
            IceUdpPortRange(41099, 1).validate_service_pools({})

    def test_cli_can_override_environment_and_invalid_defaults_fail(self):
        with patch.dict("os.environ", {"FOURPLAY_WEBRTC_ICE_UDP_PORT_START": "bad"}):
            parser = argparse.ArgumentParser()
            add_ice_port_arguments(parser)
            args = parser.parse_args(["--ice-udp-port-start", "51000", "--ice-udp-port-count", "100"])
            self.assertEqual(IceUdpPortRange.from_args(args).end, 51099)
            with self.assertRaises(ValueError):
                IceUdpPortRange.from_args(argparse.Namespace())

    def test_agent_configuration_is_applied_and_missing_support_fails_closed(self):
        class Ice:
            def __init__(self): self.props = {}; self.agent = Nice()
            def find_property(self, name): return name
            def set_property(self, name, value): self.props[name] = value
            def get_property(self, name): return self.agent if name == "agent" else self.props[name]
        ice = Ice()
        peer = argparse.Namespace(get_property=lambda _name: ice)
        configure_ice_udp_ports(peer, IceUdpPortRange(51000, 100))
        self.assertEqual(ice.props, {"min-rtp-port": 51000, "max-rtp-port": 51099, "ice-udp": True, "ice-tcp": False})
        self.assertFalse(ice.agent.upnp)
        ice.find_property = lambda name: None if name == "ice-tcp" else name
        with self.assertRaisesRegex(RuntimeError, "cannot enforce"):
            configure_ice_udp_ports(peer, IceUdpPortRange())

    def test_floating_default_agent_native_owner_is_restored_only_once(self):
        class Ice:
            def __init__(self): self.ref_count = 1; self.props = {}; self.restores = 0; self.agent = Nice()
            def _ref(self): self.ref_count += 1; self.restores += 1
            def find_property(self, name): return name
            def set_property(self, name, value): self.props[name] = value
            def get_property(self, name): return self.agent if name == "agent" else self.props[name]
        ice = Ice()
        peer = argparse.Namespace(get_property=lambda _name: ice)
        configure_ice_udp_ports(peer, IceUdpPortRange())
        configure_ice_udp_ports(peer, IceUdpPortRange())
        self.assertEqual(ice.restores, 1)
        self.assertEqual(ice.ref_count, 2)

    def test_candidate_validation_rejects_unbounded_tcp_or_missing_candidates(self):
        ports = IceUdpPortRange(51000, 100)
        validate_answer_candidates("a=candidate:1 1 UDP 123 192.168.20.68 51020 typ host\r\n", ports)
        for answer in ["v=0\r\n", "a=candidate:1 1 UDP 123 192.168.20.68 50000 typ host",
                       "a=candidate:1 1 TCP 123 192.168.20.68 9 typ host tcptype active",
                       "a=candidate:1 1 UDP 123 1.2.3.4 51000 typ srflx", "a=candidate:bad"]:
            with self.assertRaises(RuntimeError):
                validate_answer_candidates(answer, ports)

    def test_firewall_plan_validates_sources_and_does_not_expose_or_execute_secrets(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory, "4play.env")
            # This fixture is data; shell substitution must never execute.
            path.write_text("FOURPLAY_SEAT_API_TOKEN=$(exit 99)\nFOURPLAY_CONTROL_PLANE_BIND=0.0.0.0:8090\n"
                            "FOURPLAY_WEBRTC_ICE_UDP_PORT_START='51000'\nFOURPLAY_WEBRTC_ICE_UDP_PORT_COUNT=100\n", encoding="utf-8")
            settings = planner.read_settings(path, {})
            output = planner.plan(settings, ["192.168.0.186", "192.168.20.0/24", "fd00::/64"])
            self.assertIn("192.168.0.186/32", output)
            self.assertIn("51000:51099", output)
            self.assertIn("ip6 saddr fd00::/64", output)
            self.assertIn("port 8090 proto tcp", output)
            self.assertNotIn("TOKEN", output)
            self.assertNotIn("exit 99", output)
        for source in ["any", "192.168.0.1; echo unsafe", "0.0.0.0/0", "::/0", "fd00::1%zone;echo unsafe"]:
            with self.assertRaises(ValueError):
                planner.plan({}, [source])


if __name__ == "__main__":
    unittest.main()
