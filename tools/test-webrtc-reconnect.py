"""Portable ownership, bounded wait and fail-closed WAN reconnect checks."""
from types import SimpleNamespace
import ast
from pathlib import Path
import unittest
from unittest.mock import Mock
from webrtc_reconnect import release_previous_wan_peer


class ReconnectTests(unittest.TestCase):
    def test_invalid_admission_cannot_retire_an_existing_wan_peer(self):
        source = Path(__file__).with_name("webrtc-gst-raw-session-publisher.py")
        tree = ast.parse(source.read_text(encoding="utf-8"))
        manager = next(node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "MultiPeerSessionPublisher")
        scope = {"RawSessionPublisher": SimpleNamespace(read_fifo=None)}
        exec(compile(ast.fix_missing_locations(ast.Module(body=[manager], type_ignores=[])), str(source), "exec"), scope)
        publisher = scope["MultiPeerSessionPublisher"].__new__(scope["MultiPeerSessionPublisher"])
        publisher.pool = Mock()
        for admission in (None, SimpleNamespace(valid=lambda: False)):
            with self.assertRaisesRegex(RuntimeError, "not authorized"):
                publisher.answer_for_offer("v=0\nm=video", None, wan_media=True, wan_admission=admission)
        publisher.pool.remove.assert_not_called(); publisher.pool.create.assert_not_called()

    def pool(self, key="player:2", wan=True, confirmed=True, completed=True):
        cleanup = SimpleNamespace(done=Mock(), released=confirmed)
        cleanup.done.wait.return_value = completed
        peer = SimpleNamespace(wan_mode=wan, turn_cleanup=cleanup)
        pool = Mock()
        pool.snapshot.return_value = [(key, peer), ("viewer:other", object())]
        return pool, peer

    def test_only_matching_wan_player_is_removed_before_bounded_grace(self):
        pool, peer = self.pool()
        def pause(seconds):
            pool.remove.assert_called_once_with("player:2", peer)
            peer.turn_cleanup.done.wait.assert_called_once_with(5)
            self.assertEqual(seconds, 1.25)
        release_previous_wan_peer(pool, SimpleNamespace(player=2), None, pause)

    def test_spectator_replacement_does_not_remove_the_player(self):
        pool, peer = self.pool(key="viewer:mine")
        release_previous_wan_peer(pool, None, "mine", Mock())
        pool.remove.assert_called_once_with("viewer:mine", peer)

    def test_lan_or_other_slot_never_waits_or_removes(self):
        for key, wan in [("player:2", False), ("player:1", True)]:
            pool, peer = self.pool(key, wan)
            pause = Mock()
            release_previous_wan_peer(pool, SimpleNamespace(player=2), None, pause)
            pool.remove.assert_not_called(); pause.assert_not_called()
            peer.turn_cleanup.done.wait.assert_not_called()

    def test_unconfirmed_release_fails_closed_without_grace_or_new_peer(self):
        for confirmed, completed in [(False, True), (True, False)]:
            pool, peer = self.pool(confirmed=confirmed, completed=completed)
            pause = Mock()
            with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
                release_previous_wan_peer(pool, SimpleNamespace(player=2), None, pause)
            pause.assert_not_called(); pool.create.assert_not_called()


if __name__ == "__main__": unittest.main()
