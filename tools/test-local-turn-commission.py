"""Portable host commissioning guards/probes; all networking/systemd mocked."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, Mock, patch

directory = Path(__file__).resolve().parents[1] / "deploy/systemd"
sys.path.insert(0, str(directory))
spec = importlib.util.spec_from_file_location("turn_commission", directory / "start-local-only-turn.py")
commission = importlib.util.module_from_spec(spec)
spec.loader.exec_module(commission)


class CommissionTests(unittest.TestCase):
    def test_existing_managed_tables_are_not_replaced_but_other_tables_are_untouched(self):
        commission.refuse_existing_tables({"nftables": [{"table": {"family": "inet", "name": "ufw"}}]})
        for name in ("fourplay_turn", "fourplay_turn_local_only"):
            with self.assertRaises(ValueError):
                commission.refuse_existing_tables({"nftables": [{"table": {"family": "inet", "name": name}}]})

    def test_dropins_stale_daemon_or_wrong_fragment_reject_activation(self):
        for bad_index in (0, 1, 2):
            outputs = [str(commission.installer.TARGET / commission.installer.NAMES[0]), "", "no"]
            outputs[bad_index] = "unexpected"
            with patch.object(Path, "is_file", return_value=True), patch.object(Path, "is_symlink", return_value=False), patch.object(
                commission, "run", side_effect=[SimpleNamespace(stdout=value) for value in outputs]
            ):
                with self.assertRaises(ValueError): commission.check_loaded_units()

    def test_missing_installed_unit_rejects_before_systemd_query(self):
        with patch.object(Path, "is_file", return_value=False), patch.object(commission, "run") as run:
            with self.assertRaises(ValueError): commission.check_loaded_units()
            run.assert_not_called()

    def test_used_udp_relay_interval_is_rejected_before_input_policy_changes(self):
        with patch.object(Path, "read_text", return_value="header\n0: 00000000:CB21 remainder"):
            with self.assertRaisesRegex(ValueError, "relay interval is in use"): commission.check_relay_pool_unused()
        with patch.object(Path, "read_text", return_value="header\n0: 00000000:1F90 remainder"):
            commission.check_relay_pool_unused()

    def protocol(self):
        protocol = Mock()
        client = protocol.Client.return_value
        client.allocate.return_value = (0x0103, {0x0016: b"address"})
        client.exchange.return_value = (0x0108, {})
        protocol.decoded_address.return_value = ("45.24.46.155", 52000)
        protocol.packet.return_value = (b"packet", b"transaction")
        protocol.parse.return_value = (0x0017, {0x0013: b"4play-local-commission"})
        protocol.credentials.return_value = ("fixture-username", "fixture-password")
        return protocol, client

    def test_local_udp_tcp_probes_require_native_release_and_close_credentials(self):
        protocol, client = self.protocol()
        echo = MagicMock(); echo.__enter__.return_value = echo
        echo.getsockname.return_value = ("192.168.20.68", 50001)
        echo.recvfrom.return_value = (b"4play-local-commission", ("192.168.20.68", 52000))
        with patch.object(commission, "load", return_value=protocol), patch.object(commission.socket, "socket", return_value=echo), patch.object(
            commission, "owned_udp_sockets", side_effect=[{"inode": ()}, {}, {"inode2": ()}, {}]
        ) as observe, contextlib.redirect_stdout(io.StringIO()):
            commission.probe("192.168.20.68", "45.24.46.155", "fixture-secret", 42)
        self.assertEqual(observe.call_count, 4)
        self.assertEqual(client.close.call_count, 2)
        client.close.assert_called_with(True)
        self.assertEqual([call.kwargs["tcp"] for call in protocol.Client.call_args_list], [False, True])

    def test_bad_probe_source_fails_and_still_requests_release(self):
        protocol, client = self.protocol()
        echo = MagicMock(); echo.__enter__.return_value = echo
        echo.getsockname.return_value = ("192.168.20.68", 50001)
        echo.recvfrom.return_value = (b"4play-local-commission", ("192.168.20.99", 52000))
        with patch.object(commission, "load", return_value=protocol), patch.object(commission.socket, "socket", return_value=echo), patch.object(
            commission, "owned_udp_sockets", return_value={"inode": ()}
        ):
            with self.assertRaisesRegex(RuntimeError, "source mismatch"):
                commission.probe("192.168.20.68", "45.24.46.155", "fixture-secret", 42)
        client.close.assert_called_once_with(True)

    def test_guard_has_no_global_flush_and_retains_established_udp_replies(self):
        rules = commission.rules()
        self.assertIn("table inet fourplay_turn_local_only", rules)
        self.assertNotIn("flush", rules); self.assertNotIn("delete", rules)
        self.assertLess(rules.index("ct state established,related return"), rules.index("udp dport"))
        self.assertIn('iifname "lo" return', rules)
        self.assertIn("tcp dport 3478 counter drop", rules)

    def test_plan_never_reads_root_bundle_or_runs_services(self):
        with patch.object(sys, "argv", ["commission", "--describe"]), patch.object(commission.installer, "verify_bundle") as verify, patch.object(
            commission, "run"
        ) as run, contextlib.redirect_stdout(io.StringIO()):
            commission.main(); verify.assert_not_called(); run.assert_not_called()

    @unittest.skipUnless(sys.platform == "linux", "Linux startup flow with every privileged effect mocked")
    def test_failed_live_probe_requests_stop_only_for_relay_and_never_removes_restrictions(self):
        import pwd
        account = SimpleNamespace(pw_uid=105, pw_gid=108, pw_name="turnserver")
        def execute(args, **kwargs):
            if "ruleset" in args: return SimpleNamespace(stdout='{"nftables": []}')
            if "--property=MainPID" in args: return SimpleNamespace(stdout="42")
            return SimpleNamespace(stdout="")
        def link(path):
            return commission.installer.bundle.candidate.BINARY.as_posix() if str(path).endswith("/exe") else "net:[1]"
        with contextlib.ExitStack() as stack:
            def replace(target, name, **kwargs): return stack.enter_context(patch.object(target, name, **kwargs))
            replace(sys, "argv", new=["commission", "--apply"])
            replace(commission.os, "geteuid", return_value=0)
            replace(commission.os, "getgrouplist", return_value=[108])
            replace(pwd, "getpwnam", return_value=account)
            replace(commission.os, "readlink", side_effect=link)
            replace(commission.installer, "verify_bundle", return_value={"fixture": b"reviewed"})
            replace(commission.installer, "check_targets")
            replace(commission.installer, "check_inactive")
            replace(commission, "check_loaded_units")
            replace(commission, "check_uid_unused")
            replace(commission, "check_relay_pool_unused")
            replace(commission.installer.stage, "read_private", return_value=json.dumps({"local_ip":"192.168.20.68", "public_ip":"45.24.46.155"}).encode())
            replace(commission.installer.stage, "existing_secret", return_value="fixture-secret")
            replace(commission.socket, "socket", return_value=MagicMock())
            replace(commission.socket, "create_connection", return_value=MagicMock())
            replace(commission, "load", return_value=Mock())
            replace(commission, "probe", side_effect=RuntimeError("probe fixture failed"))
            run = replace(commission, "run", side_effect=execute)
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaisesRegex(RuntimeError, "probe fixture failed"):
                commission.main()
            commands = [call.args[0] for call in run.call_args_list]
            self.assertIn(["/usr/bin/systemctl", "start", "4play-turn.service"], commands)
            self.assertIn(["/usr/bin/systemctl", "stop", "4play-turn.service"], commands)
            self.assertFalse(any("delete" in args or "flush" in args or "enable" in args for args in commands))
            self.assertFalse(any("4play-control-plane.service" in args for args in commands))


if __name__ == "__main__": unittest.main()
