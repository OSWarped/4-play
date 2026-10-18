"""Pinned build safety tests, no network/compiler/install needed."""
import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "deploy/systemd/build-private-coturn.py"
spec = importlib.util.spec_from_file_location("private_coturn_build", SCRIPT)
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class BuildTests(unittest.TestCase):
    def test_dependencies_report_missing_headers(self):
        def run(args, **_kwargs): return SimpleNamespace(returncode=int(args[-1] != "openssl"))
        self.assertEqual(builder.missing_prerequisites(lambda name: "/usr/bin/"+name, run), ["libevent-dev", "libsqlite3-dev"])

    def test_missing_pkg_config_does_not_run_commands(self):
        def fail(*_args, **_kwargs): raise AssertionError("must not execute missing tool")
        self.assertIn("pkg-config", builder.missing_prerequisites(lambda _name: None, fail))

    def test_changed_tag_fails_closed(self):
        with self.assertRaises(RuntimeError):
            builder.verify_checkout(Path("unused"), lambda *_args, **_kwargs: SimpleNamespace(stdout="different-commit"))
        builder.verify_checkout(Path("unused"), lambda *_args, **_kwargs: SimpleNamespace(stdout=builder.COMMIT+"\n"))

    def test_third_party_build_does_not_inherit_service_secrets_or_compiler_hooks(self):
        with patch.dict(builder.os.environ, {"PATH":"/usr/bin", "FOURPLAY_SEAT_API_TOKEN":"private", "CC":"injected", "LD_PRELOAD":"injected"}, clear=True):
            env = builder.build_environment()
        self.assertEqual(env["PATH"], "/usr/bin")
        for name in ["FOURPLAY_SEAT_API_TOKEN", "CC", "LD_PRELOAD"]:
            self.assertNotIn(name, env)
        self.assertEqual(env["TURN_NO_PROMETHEUS"], "1")


if __name__ == "__main__": unittest.main()
