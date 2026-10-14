"""Client-address trust and legacy fallback regression checks; no GI required."""
import unittest
import ast
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from types import SimpleNamespace
import sys
import threading
import traceback
from urllib.request import Request, urlopen
from urllib.error import HTTPError
from webrtc_client_address import browser_host_for_request
from webrtc_browser_input import InputAuthorization
from webrtc_peer_pool import PeerCapacityError


class AddressTests(unittest.TestCase):
    def test_trusted_client_address_overrides_fixed_reference_pc(self):
        for client in ["192.168.20.10", "192.168.0.186"]:
            self.assertEqual(browser_host_for_request({"x-4play-browser-ip": client}, "127.0.0.1", "192.168.20.10"), client)

    def test_direct_lan_caller_cannot_spoof_forwarding_header(self):
        self.assertEqual(browser_host_for_request({"x-4play-browser-ip": "10.0.0.99"}, "192.168.0.186", "192.168.20.10"), "192.168.0.186")

    def test_loopback_without_metadata_uses_only_explicit_legacy_fallback(self):
        self.assertIsNone(browser_host_for_request({}, "127.0.0.1"))
        self.assertEqual(browser_host_for_request({}, "::1", "192.168.20.10"), "192.168.20.10")

    def test_valid_ipv6_and_mapped_ipv4_are_normalized(self):
        self.assertEqual(browser_host_for_request({"x-4play-browser-ip": "::ffff:192.168.0.186"}, "::ffff:127.0.0.1"), "192.168.0.186")
        self.assertEqual(browser_host_for_request({"x-4play-browser-ip": "fd00::186"}, "::1"), "fd00::186")

    def test_invalid_trusted_address_does_not_fall_back_to_wrong_pc(self):
        for address in ["", "host.local", "192.168.0.186:8080", "127.0.0.1", "0.0.0.0", "224.0.0.1", "255.255.255.255", "fe80::1%eth0"]:
            with self.assertRaises(ValueError):
                browser_host_for_request({"x-4play-browser-ip": address}, "127.0.0.1", "192.168.20.10")


class HttpAddressTests(unittest.TestCase):
    def test_shipped_raw_http_handler_uses_trusted_metadata_and_rejects_invalid_ip(self):
        # Execute the actual handler without loading platform-specific GI bindings.
        script = Path(__file__).with_name("webrtc-gst-raw-session-publisher.py")
        tree = ast.parse(script.read_text(encoding="utf-8"))
        handler = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "request_handler")
        namespace = dict(BaseHTTPRequestHandler=BaseHTTPRequestHandler, browser_host_for_request=browser_host_for_request,
                         InputAuthorization=InputAuthorization, PeerCapacityError=PeerCapacityError,
                         MultiPeerSessionPublisher=type("MultiPeer", (), {}), sys=sys, traceback=traceback)
        module = ast.Module(body=[ast.ImportFrom(module="__future__", names=[ast.alias(name="annotations")], level=0), handler], type_ignores=[])
        exec(compile(ast.fix_missing_locations(module), str(script), "exec"), namespace)
        addresses = []

        def answer(offer, host, **_options):
            addresses.append(host)
            return "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n"

        publisher = SimpleNamespace(args=SimpleNamespace(session_id="address-check", browser_host_ip="192.168.20.10"), answer_for_offer=answer)
        server = ThreadingHTTPServer(("127.0.0.1", 0), namespace["request_handler"](publisher))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        url = f"http://127.0.0.1:{server.server_port}/sessions/address-check/offer"
        try:
            for source in ["192.168.0.186", "192.168.20.10", None]:
                headers = {"Content-Type": "application/sdp"}
                if source:
                    headers["x-4play-browser-ip"] = source
                with urlopen(Request(url, data=b"v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n", headers=headers), timeout=3) as response:
                    self.assertEqual(response.status, 200)
            self.assertEqual(addresses, ["192.168.0.186", "192.168.20.10", "192.168.20.10"])
            with self.assertRaises(HTTPError) as rejected:
                urlopen(Request(url, data=b"v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n", headers={"x-4play-browser-ip": "invalid"}), timeout=3)
            self.assertEqual(rejected.exception.code, 500)
            self.assertEqual(len(addresses), 3)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)


if __name__ == "__main__":
    unittest.main()
