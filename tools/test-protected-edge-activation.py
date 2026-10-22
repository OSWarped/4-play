"""Exact-target nginx activation/rollback tests; never touches live nginx."""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("protected_activation", ROOT / "deploy/nginx/activate-protected-edge.py")
activation = importlib.util.module_from_spec(spec); spec.loader.exec_module(activation)


@unittest.skipUnless(os.name == "posix", "Linux symlink semantics required")
class ActivationTests(unittest.TestCase):
    @contextlib.contextmanager
    def fixture(self):
        with tempfile.TemporaryDirectory() as temporary, contextlib.ExitStack() as stack:
            root = Path(temporary)
            site, enabled = root / "site", root / "enabled"
            site.write_bytes(activation.staging.render(True).encode()); enabled.symlink_to(site)
            marker = root / "unrelated"; marker.write_bytes(b"keep")
            for name, value in (("SITE", site), ("ENABLED", enabled)):
                stack.enter_context(patch.object(activation.staging, name, value))
            # Fixture ownership substitutes for the real root metadata checks.
            def preflight():
                if not enabled.is_symlink() or enabled.readlink() != site: raise ValueError("wrong link")
                content = site.read_bytes()
                if content not in (activation.staging.render(True).encode(), activation.candidate()): raise ValueError("unknown config")
                return content
            stack.enter_context(patch.object(activation, "preflight", side_effect=preflight))
            run = stack.enter_context(patch.object(activation, "run"))
            yield site, enabled, marker, run

    def test_activation_preserves_other_files_link_and_private_keys(self):
        with self.fixture() as (site, enabled, marker, run):
            verify = unittest.mock.Mock()
            activation.switch(activation.candidate(), verify)
            self.assertEqual(site.read_bytes(), activation.candidate())
            self.assertEqual(enabled.readlink(), site); self.assertEqual(marker.read_bytes(), b"keep")
            verify.assert_called_once()
            self.assertEqual([call.args[0] for call in run.call_args_list], [
                ["/usr/sbin/nginx", "-t"], ["/usr/sbin/nginx", "-t"], ["/usr/bin/systemctl", "reload", "nginx"]])

    def test_syntax_reload_and_postcheck_failures_restore_staging(self):
        for failure in (2, 3, "postcheck"):
            with self.subTest(failure=failure), self.fixture() as (site, enabled, marker, run), contextlib.redirect_stdout(io.StringIO()):
                calls = 0
                def command(_args):
                    nonlocal calls
                    calls += 1
                    if calls == failure: raise RuntimeError("fixture")
                run.side_effect = command
                verify = unittest.mock.Mock(side_effect=RuntimeError("postcheck") if failure == "postcheck" else None)
                with self.assertRaises(RuntimeError): activation.switch(activation.candidate(), verify)
                self.assertEqual(site.read_bytes(), activation.staging.render(True).encode())
                self.assertEqual(enabled.readlink(), site); self.assertEqual(marker.read_bytes(), b"keep")
                self.assertEqual(run.call_args_list[-1].args[0], ["/usr/bin/systemctl", "reload", "nginx"])

    def test_initial_nginx_failure_leaves_files_untouched(self):
        with self.fixture() as (site, _, _, run):
            run.side_effect = RuntimeError("existing nginx failure")
            with self.assertRaises(RuntimeError): activation.switch(activation.candidate(), lambda: None)
            self.assertEqual(site.read_bytes(), activation.staging.render(True).encode())

    def test_idempotence_and_explicit_rollback(self):
        with self.fixture() as (site, _, _, run):
            activation.switch(activation.candidate(), lambda: None)
            run.reset_mock()
            activation.switch(activation.candidate(), lambda: None)
            self.assertEqual(run.call_count, 1)  # syntax check only; no reload
            activation.switch(activation.staging.render(True).encode(), lambda: None)
            self.assertEqual(site.read_bytes(), activation.staging.render(True).encode())

    def test_unknown_configuration_and_wrong_link_refuse_all_mutation(self):
        for broken in ("config", "link"):
            with self.fixture() as (site, enabled, marker, run):
                if broken == "config": site.write_bytes(b"custom")
                else: enabled.unlink(); enabled.symlink_to(marker)
                before = site.read_bytes()
                with self.assertRaises(ValueError): activation.switch(activation.candidate(), lambda: None)
                self.assertEqual(site.read_bytes(), before); run.assert_not_called()


class PlanTests(unittest.TestCase):
    def test_plan_is_unprivileged_and_does_not_run_checks(self):
        with patch.object(sys, "argv", ["activate"]), patch.object(activation, "run") as run, patch.object(activation, "preflight") as preflight, contextlib.redirect_stdout(io.StringIO()):
            activation.main(); run.assert_not_called(); preflight.assert_not_called()

    @unittest.skipUnless(sys.platform == "linux", "Linux root flow with privileges mocked")
    def test_failed_live_admission_gate_prevents_switch(self):
        with patch.object(sys, "argv", ["activate", "--apply"]), patch.object(activation.os, "geteuid", return_value=0), patch.object(
            activation, "preflight", return_value=activation.staging.render(True).encode()
        ), patch.object(activation, "run", side_effect=RuntimeError("gate failed")), patch.object(activation, "switch") as switch:
            with self.assertRaises(RuntimeError): activation.main()
            switch.assert_not_called()

    @unittest.skipUnless(sys.platform == "linux", "Linux root flow with privileges mocked")
    def test_restore_staging_does_not_depend_on_application_health(self):
        with patch.object(sys, "argv", ["activate", "--apply", "--restore-staging"]), patch.object(activation.os, "geteuid", return_value=0), patch.object(
            activation, "preflight", return_value=activation.candidate()
        ), patch.object(activation, "run") as run, patch.object(activation, "switch") as switch:
            activation.main(); run.assert_not_called()
            switch.assert_called_once_with(activation.staging.render(True).encode(), activation.verify_staging)


if __name__ == "__main__": unittest.main()
