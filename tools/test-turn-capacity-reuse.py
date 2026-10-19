"""Portable checks for the bounded native bandwidth-reuse probe."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("capacity_protocol", Path(__file__).with_name("test-coturn-loopback.py"))
protocol = importlib.util.module_from_spec(spec)
spec.loader.exec_module(protocol)


class CapacityTests(unittest.TestCase):
    def test_twelve_cycles_observe_release_and_alternate_transport(self):
        client = Mock()
        client.allocate.return_value = (0x0103, {0x0016: protocol.xor_address("127.0.0.1", 62001)})
        with patch.object(protocol, "Client", return_value=client) as create, patch(
            "turn_socket_observation.owned_udp_sockets", side_effect=[{"123": ("udp", "address", 62001)}, {}] * 12
        ) as observe:
            protocol.check_capacity_reuse(3478, "fixture-secret", 42)
        self.assertEqual(create.call_count, 12)
        self.assertEqual([call.kwargs["tcp"] for call in create.call_args_list], [False, True] * 6)
        self.assertEqual(client.close.call_count, 12)
        client.close.assert_called_with(True)
        self.assertEqual(observe.call_count, 24)

    def test_rejected_allocation_fails_and_does_not_request_release(self):
        client = Mock()
        client.allocate.return_value = (0x0113, {})
        with patch.object(protocol, "Client", return_value=client):
            with self.assertRaisesRegex(AssertionError, "cycle 1"):
                protocol.check_capacity_reuse(3478, "fixture-secret", 42)
        client.close.assert_called_once_with(False)

    def test_missing_native_socket_is_not_a_release_pass(self):
        client = Mock()
        client.allocate.return_value = (0x0103, {0x0016: protocol.xor_address("127.0.0.1", 62001)})
        with patch.object(protocol, "Client", return_value=client), patch(
            "turn_socket_observation.owned_udp_sockets", return_value={}
        ):
            with self.assertRaisesRegex(AssertionError, "observed relay socket"):
                protocol.check_capacity_reuse(3478, "fixture-secret", 42)
        client.close.assert_called_once_with(True)

    def test_unreadable_native_sockets_fail_closed_and_release(self):
        client = Mock()
        client.allocate.return_value = (0x0103, {0x0016: protocol.xor_address("127.0.0.1", 62001)})
        with patch.object(protocol, "Client", return_value=client), patch(
            "turn_socket_observation.owned_udp_sockets", side_effect=PermissionError
        ):
            with self.assertRaises(PermissionError):
                protocol.check_capacity_reuse(3478, "fixture-secret", 42)
        client.close.assert_called_once_with(True)


if __name__ == "__main__": unittest.main()
