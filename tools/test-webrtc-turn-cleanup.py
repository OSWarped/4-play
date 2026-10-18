"""Deterministic async TURN shutdown tests; no GI/network required."""
from types import SimpleNamespace
import unittest
from unittest.mock import Mock

from webrtc_turn_cleanup import TurnCleanup, _pending


class CleanupTests(unittest.TestCase):
    def fixture(self):
        pipeline, peer, glib, nice = Mock(), Mock(), Mock(), Mock()
        glib.timeout_add.return_value = 42
        task = TurnCleanup(pipeline, peer, SimpleNamespace(State=SimpleNamespace(NULL="null")), glib, nice)
        self.addCleanup(lambda: task.finish(False))
        return task, pipeline, glib, nice

    def test_only_disposes_after_native_completion_and_keeps_owner_alive(self):
        task, pipeline, glib, nice = self.fixture()
        glib.idle_add.assert_called_once_with(task.start)
        pipeline.set_state.assert_not_called()
        task.start()
        self.assertIn(task, _pending)
        pipeline.set_state.assert_not_called()
        nice.Agent.close_async.assert_called_once_with(task.agent, task.completed, None)
        task.completed(None, None, None)
        self.assertTrue(task.done.is_set())
        self.assertTrue(task.released)
        pipeline.set_state.assert_called_once_with("null")
        glib.source_remove.assert_called_once_with(42)
        self.assertNotIn(task, _pending)
        task.completed()
        pipeline.set_state.assert_called_once()

    def test_timeout_then_late_completion_is_safe_and_not_success(self):
        task, pipeline, glib, nice = self.fixture()
        task.start()
        task.timed_out()
        task.completed()
        self.assertTrue(task.done.is_set())
        self.assertFalse(task.released)
        glib.source_remove.assert_not_called()
        pipeline.set_state.assert_called_once()

    def test_native_exception_does_not_hold_pipeline_or_escape(self):
        task, pipeline, glib, nice = self.fixture()
        nice.Agent.close_async.side_effect = RuntimeError("private-native-error")
        task.start()
        self.assertTrue(task.done.is_set())
        self.assertFalse(task.released)
        pipeline.set_state.assert_called_once()
        self.assertIsNone(task.agent)


if __name__ == "__main__": unittest.main()
