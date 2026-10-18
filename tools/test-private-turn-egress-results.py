"""Portable result/worker regressions; no privileged imports or network access."""
import ast
import errno
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import Mock

source = Path(__file__).with_name("test-private-turn-egress.py")
tree = ast.parse(source.read_text(encoding="utf-8"))
selected = [node for node in tree.body if
            isinstance(node, ast.FunctionDef) and node.name == "validate_udp_result" or
            isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id == "WORKER" for target in node.targets)]
namespace = {}
exec(compile(ast.Module(body=selected, type_ignores=[]), str(source), "exec"), namespace)
validate = namespace["validate_udp_result"]


class ResultTests(unittest.TestCase):
    def test_silent_drop_and_explicit_permission_denial_are_both_blocked(self):
        for code in (0, 77):
            validate(code, None, False)

    def test_allowed_requires_success_and_exact_delivery(self):
        validate(0, b"4play-egress-fixture", True)
        for code, data in [(77, None), (0, None), (77, b"4play-egress-fixture"), (0, b"wrong")]:
            with self.assertRaises(AssertionError): validate(code, data, True)

    def test_blocked_still_rejects_delivery_and_unexpected_worker_errors(self):
        for code in (0, 77):
            for data in (b"4play-egress-fixture", b"wrong", b""):
                with self.assertRaises(AssertionError): validate(code, data, False)
        for code in (1, 2, -9):
            for allowed in (False, True):
                with self.assertRaises(AssertionError): validate(code, None, allowed)

    def run_worker(self, send_error=None, bind_error=None):
        sock = Mock()
        sock.sendto.side_effect = send_error
        sock.bind.side_effect = bind_error
        fake_socket = SimpleNamespace(socket=Mock(return_value=sock), AF_INET=2, AF_INET6=10, SOCK_DGRAM=2)
        def exit_worker(code): raise SystemExit(code)
        fake_sys = SimpleNamespace(argv=["worker", "192.168.20.68", "8080", "52010", "4"], exit=exit_worker)
        modules = {"socket":fake_socket, "sys":fake_sys, "errno":errno}
        def import_module(name, *args): return modules[name]
        return sock, lambda: exec(namespace["WORKER"], {"__builtins__":dict(vars(__import__("builtins")), __import__=import_module)})

    def test_worker_reports_only_send_permission_errors_as_expected_denial(self):
        for number in (errno.EPERM, errno.EACCES):
            sock, run = self.run_worker(send_error=OSError(number, "blocked"))
            with self.assertRaises(SystemExit) as result: run()
            self.assertEqual(result.exception.code, 77)
            sock.close.assert_called_once()

    def test_bind_and_other_send_errors_are_not_mislabeled_as_firewall_blocks(self):
        for send, bind in [(OSError(errno.ENETUNREACH, "route"), None), (None, OSError(errno.EACCES, "bind"))]:
            _sock, run = self.run_worker(send_error=send, bind_error=bind)
            with self.assertRaises(OSError): run()


if __name__ == "__main__": unittest.main()
