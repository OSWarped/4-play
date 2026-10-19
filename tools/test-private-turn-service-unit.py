"""Portable transient service safety checks; no sudo/systemd/network needed."""
import importlib.util
from pathlib import Path, PurePosixPath
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("private_turn_service_test", Path(__file__).with_name("test-private-turn-service.py"))
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class ServiceTests(unittest.TestCase):
    def command(self, unit="4play-turn-validation-" + "a"*32 + ".service"):
        return fixture.transient_command(unit, PurePosixPath("/proc/123/ns/net"), PurePosixPath("/tmp/private/config"),
                                         PurePosixPath("/tmp/private/log"), PurePosixPath("/opt/4play-coturn/4.18.0/turnserver"))

    def test_all_security_properties_are_copied_from_reviewed_service(self):
        command = self.command()
        supplied = dict(arg.removeprefix("--property=").split("=", 1) for arg in command if arg.startswith("--property="))
        original = fixture.service_properties(fixture.service_units()["4play-turn.service"])
        overrides = {"ExecStart", "RuntimeDirectory", "LoadCredential", "Restart", "Type"}
        for key, value in original.items():
            if key not in overrides:
                self.assertEqual(supplied[key], value, key)
        self.assertEqual(supplied["NetworkNamespacePath"], "/proc/123/ns/net")
        self.assertEqual(supplied["Restart"], "no")
        self.assertEqual(supplied["RuntimeMaxSec"], "90s")
        self.assertIn("--collect", command)
        self.assertNotIn("/etc/4play-turn/turnserver.conf", " ".join(command))
        self.assertEqual(command[-3:], ["/opt/4play-coturn/4.18.0/turnserver", "-c",
            "/run/credentials/4play-turn-validation-" + "a"*32 + ".service/turnserver.conf"])

    def test_production_or_ambiguous_unit_names_cannot_be_started_or_stopped(self):
        for unit in ("coturn.service", "4play-turn.service", "../bad", "4play-turn-validation-test.service"):
            with self.assertRaises(ValueError): self.command(unit)

    def test_duplicate_properties_are_rejected(self):
        with self.assertRaises(ValueError): fixture.service_properties("[Service]\nUser=turnserver\nUser=root\n")

    def test_process_must_have_expected_identity_and_effective_confinement(self):
        status = "Uid:\t105 105 105 105\nNoNewPrivs:\t1\n" + "".join(f"{name}:\t0000000000000000\n" for name in ("CapEff", "CapPrm", "CapInh", "CapBnd", "CapAmb"))
        links = {"/proc/42/ns/net":"net:[2]", "/proc/123/ns/net":"net:[2]",
                 "/proc/42/ns/mnt":"mnt:[3]", "/proc/1/ns/mnt":"mnt:[1]"}
        with patch.object(Path, "read_text", return_value=status), patch.object(fixture.os, "readlink", side_effect=lambda p:links[str(p)]):
            fixture.check_process(42, PurePosixPath("/proc/123/ns/net"), 105)
        for modified in [status.replace("105", "0"), status.replace("0000000000000000", "0000000000000001"), status.replace("NoNewPrivs:\t1", "NoNewPrivs:\t0")]:
            with patch.object(Path, "read_text", return_value=modified):
                with self.assertRaises(RuntimeError): fixture.check_process(42, PurePosixPath("/proc/123/ns/net"), 105)
        for path, value in [("/proc/42/ns/net", "net:[1]"), ("/proc/42/ns/mnt", "mnt:[1]")]:
            changed = links | {path:value}
            with patch.object(Path, "read_text", return_value=status), patch.object(fixture.os, "readlink", side_effect=lambda p:changed[str(p)]):
                with self.assertRaises(RuntimeError): fixture.check_process(42, PurePosixPath("/proc/123/ns/net"), 105)

    def test_protocol_client_can_target_fixture_address_without_changing_default(self):
        protocol = fixture.load("protocol_unit_test", Path(__file__).with_name("test-coturn-loopback.py"))
        with patch.object(protocol.socket, "socket") as make:
            protocol.Client(3478)
            make.return_value.connect.assert_called_with(("127.0.0.1", 3478))
            protocol.Client(3478, host="192.168.20.68")
            make.return_value.connect.assert_called_with(("192.168.20.68", 3478))

    def test_release_requires_zero_lifetime_ack_and_closes_on_failure(self):
        protocol = fixture.load("protocol_release_test", Path(__file__).with_name("test-coturn-loopback.py"))
        with patch.object(protocol.socket, "socket") as make:
            for lifetime in (b"\0\0\0\0", b"\0\0\0\x3c", None):
                client = protocol.Client(3478)
                with patch.object(client, "exchange", return_value=(0x0104, {0x000d:lifetime})):
                    if lifetime == b"\0\0\0\0":
                        client.close(True)
                    else:
                        with self.assertRaises(AssertionError): client.close(True)
                make.return_value.close.assert_called()

    def test_media_rejects_host_namespace_before_reading_config(self):
        with patch.object(fixture.os, "readlink", return_value="net:[1]"), patch.object(Path, "read_text") as read:
            with self.assertRaisesRegex(RuntimeError, "host network namespace"):
                fixture.media_checks(Path("unused"), 42)
            read.assert_not_called()

    def test_media_rejects_relay_in_another_namespace(self):
        links = {"/proc/self/ns/net":"net:[2]", "/proc/1/ns/net":"net:[1]", "/proc/42/ns/net":"net:[3]"}
        with patch.object(fixture.os, "readlink", side_effect=lambda p:links[str(p)]), patch.object(Path, "read_text") as read:
            with self.assertRaisesRegex(RuntimeError, "outside this private namespace"):
                fixture.media_checks(Path("unused"), 42)
            read.assert_not_called()

    def test_media_rejects_production_nat_binding_and_invalid_secrets(self):
        links = {"/proc/self/ns/net":"net:[2]", "/proc/1/ns/net":"net:[1]", "/proc/42/ns/net":"net:[2]"}
        from private_turn_bundle import turn_config
        base = turn_config("192.168.20.68", "45.24.46.155", "a"*64)
        direct = base.replace("relay-ip=192.168.20.68\n", "relay-ip=45.24.46.155\n").replace(
            "external-ip=45.24.46.155/192.168.20.68\n", "external-ip=45.24.46.155/45.24.46.155\n")
        for content in (base, direct.replace("static-auth-secret=" + "a"*64, "static-auth-secret=invalid"),
                        direct + "\nstatic-auth-secret=" + "b"*64 + "\n"):
            with patch.object(fixture.os, "readlink", side_effect=lambda p:links[str(p)]), patch.object(Path, "read_text", return_value=content):
                with self.assertRaises(RuntimeError): fixture.media_checks(Path("unused"), 42)


if __name__ == "__main__": unittest.main()
