"""Pure renderer checks; no service/firewall mutations."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("turn_bundle", Path(__file__).resolve().parents[1] / "deploy/systemd/private_turn_bundle.py")
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)


class BundleTests(unittest.TestCase):
    def test_invalid_addresses_and_uids_cannot_inject_configuration(self):
        for uid in [0, -1, True, "105\naccept"]:
            with self.assertRaises(ValueError): bundle.egress_rules(uid, "192.168.20.68", "45.24.46.155")
        for local, public in [("127.0.0.1", "45.24.46.155"), ("192.168.20.68", "192.168.0.1"),
                              ("::1", "45.24.46.155"), ("192.168.20.68\naccept", "45.24.46.155")]:
            with self.assertRaises(ValueError): bundle.egress_rules(105, local, public)

    def test_uid_guard_precedes_restrictions_and_internal_exception_precedes_denial(self):
        rules = bundle.egress_rules(105, "192.168.20.68", "45.24.46.155")
        self.assertLess(rules.index("meta skuid != 105 return"), rules.index("counter drop"))
        self.assertLess(rules.index("udp dport 50000-50999"), rules.index("192.168.0.0/16"))
        self.assertLess(rules.index("192.168.0.0/16"), rules.index("udp sport 52000-52199"))
        self.assertNotIn("flush ruleset", rules)
        self.assertNotIn("hook input", rules)

    def test_secret_is_only_in_credential_config_not_units(self):
        secret = "a" * 64
        config = bundle.turn_config("192.168.20.68", "45.24.46.155", secret)
        self.assertIn(f"static-auth-secret={secret}", config)
        self.assertNotIn("allow-loopback-peers", config)
        self.assertIn("no-tcp-relay", config)
        units = bundle.service_units()
        self.assertNotIn(secret, str(units))
        self.assertIn("Requires=4play-turn-egress.service", units["4play-turn.service"])
        self.assertIn("User=turnserver", units["4play-turn.service"])
        self.assertNotIn("ExecStop=", units["4play-turn-egress.service"])
        with self.assertRaises(ValueError): bundle.turn_config("192.168.20.68", "45.24.46.155", "unsafe\n")


if __name__ == "__main__": unittest.main()
