"""Portable regression: namespace-wide UDP entries are not per-process owners."""
from pathlib import Path
import unittest
from unittest.mock import patch

from turn_socket_observation import owned_udp_sockets


def row(port, inode):
    return f"0: 4400A8C0:{port:04X} 00000000:0000 07 0:0 0:0 0 105 0 {inode} 2 0 0\n"


class ObservationTests(unittest.TestCase):
    def observe(self, links, text, start=52000, end=52199):
        with patch.object(Path, "iterdir", return_value=list(links)), \
             patch("turn_socket_observation.os.readlink", side_effect=lambda path:links[path]), \
             patch.object(Path, "read_text", side_effect=lambda self=None: "header\n" + text):
            return owned_udp_sockets(42, start, end)

    def test_unrelated_namespace_sockets_are_excluded_even_in_relay_range(self):
        sockets = self.observe({"fd1":"socket:[111]", "fd2":"/tmp/log"}, row(52010, "111") + row(52011, "222") + row(3478, "111"))
        self.assertEqual(set(sockets), {"111"})
        self.assertEqual(sockets["111"][2], 52010)

    def test_only_the_advertised_port_is_selected_not_an_auxiliary_socket(self):
        sockets = self.observe({"fd1":"socket:[111]", "fd2":"socket:[222]"},
                               row(52010, "111") + row(52011, "222"), start=52010, end=52010)
        self.assertEqual(set(sockets), {"111"})

    def test_released_allocation_is_absent_even_if_udp_table_has_a_stale_entry(self):
        self.assertEqual(self.observe({"fd2":"socket:[222]"}, row(52010, "111") + row(3478, "222")), {})

    def test_visibility_errors_do_not_count_as_successful_release(self):
        with patch.object(Path, "iterdir", side_effect=PermissionError("fixture")):
            with self.assertRaises(PermissionError): owned_udp_sockets(42, 52000, 52199)

    def test_invalid_pid_or_range_is_rejected(self):
        for pid, start, end in [(0, 52000, 52199), (True, 52000, 52199), (42, 0, 52199), (42, 52000, 51999)]:
            with self.assertRaises(ValueError): owned_udp_sockets(pid, start, end)


if __name__ == "__main__": unittest.main()
