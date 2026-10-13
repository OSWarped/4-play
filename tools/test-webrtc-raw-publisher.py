"""Regression checks for whole-frame input and encoder timestamp stability.

Run with python3 tools/test-webrtc-raw-publisher.py. The encoder check requires
Linux GStreamer bindings; the framing/timing checks also run on Windows.
"""
import argparse
import ast
import importlib.util
from pathlib import Path
import threading
import time
import unittest
from unittest.mock import Mock, patch
from types import SimpleNamespace


SCRIPT = Path(__file__).with_name("webrtc-gst-raw-session-publisher.py")
tree = ast.parse(SCRIPT.read_text(encoding="utf-8"))
publisher_class = next(node for node in tree.body if isinstance(node, ast.ClassDef))
namespace = {}
test_module = ast.Module(body=[ast.ImportFrom(module="__future__", names=[ast.alias(name="annotations")], level=0), publisher_class], type_ignores=[])
exec(compile(ast.fix_missing_locations(test_module), str(SCRIPT), "exec"), namespace)
Publisher = namespace["RawSessionPublisher"]


class InputTests(unittest.TestCase):
    def test_peer_cleanup_deadlines_and_connection_recovery(self):
        publisher = Publisher.__new__(Publisher)
        publisher.closed = False
        publisher.pipeline_failed = False
        publisher.input_authorization = None
        publisher.created_at = 10
        publisher.unhealthy_since = None
        publisher.webrtc = Mock()
        publisher.webrtc.get_property.return_value = SimpleNamespace(value_nick="connecting")
        self.assertFalse(publisher.expired(39))
        self.assertTrue(publisher.expired(40))
        publisher.webrtc.get_property.return_value = SimpleNamespace(value_nick="connected")
        self.assertFalse(publisher.expired(500))
        publisher.webrtc.get_property.return_value = SimpleNamespace(value_nick="disconnected")
        self.assertFalse(publisher.expired(500))
        self.assertTrue(publisher.expired(515))
        publisher.webrtc.get_property.return_value = SimpleNamespace(value_nick="connected")
        self.assertFalse(publisher.expired(516))
        self.assertIsNone(publisher.unhealthy_since)
        publisher.webrtc.get_property.return_value = SimpleNamespace(value_nick="failed")
        self.assertTrue(publisher.expired(517))
        publisher.webrtc.get_property.return_value = SimpleNamespace(value_nick="connected")
        publisher.input_authorization = SimpleNamespace(peer_id="old")
        publisher.input_relay = SimpleNamespace(authorization=SimpleNamespace(peer_id="new"))
        self.assertTrue(publisher.expired(518))

    def test_partial_reads_preserve_frame_boundaries(self):
        publisher = Publisher.__new__(Publisher)
        publisher.reader_stop = threading.Event()
        blocks = []

        def capture(label, data):
            blocks.append((label, bytes(data)))
            if len(blocks) == 2:
                publisher.reader_stop.set()

        publisher.push_media_block = capture
        fake_os = Mock()
        fake_os.O_RDONLY = 0
        fake_os.O_NONBLOCK = 1
        fake_os.read.side_effect = [b"", b"ab", b"cd", b"e", b"fgh"]
        namespace.update(os=fake_os, select=Mock(), GLib=Mock())
        namespace["select"].select.return_value = ([1], [], [])
        publisher.read_fifo("video", "unused", 4)
        self.assertEqual(blocks, [("video", b"abcd"), ("video", b"efgh")])
        fake_os.close.assert_called_once()

    def test_clock_never_moves_back_and_audio_uses_same_epoch(self):
        publisher = Publisher.__new__(Publisher)
        publisher.media_lock = threading.Lock()
        publisher.media_epoch_ns = 100
        publisher.last_pts = {}
        publisher.audio_next_pts = None
        publisher.args = argparse.Namespace(fps_num=60000, fps_den=1000)
        source = Mock()
        source.emit.return_value = "ok"
        publisher.pipeline = Mock()
        publisher.pipeline.get_by_name.return_value = source
        fake_gst = Mock()
        fake_gst.SECOND = 1_000_000_000
        fake_gst.CLOCK_TIME_NONE = -1
        fake_gst.FlowReturn.OK = "ok"
        fake_gst.Buffer.new_allocate.side_effect = lambda *args: Mock()
        namespace.update(Gst=fake_gst, time=time)
        with patch("time.monotonic_ns", side_effect=[200, 200, 199, 210]):
            for label in ["video", "video", "video", "audio"]:
                publisher.push_media_block(label, bytearray(3840))
        buffers = [call.args[1] for call in source.emit.call_args_list]
        self.assertEqual([buffer.pts for buffer in buffers], [100, 101, 102, 110])
        self.assertEqual(buffers[0].duration, 16_666_666)
        self.assertEqual(buffers[3].duration, 20_000_000)
        publisher.media_epoch_ns = None
        publisher.push_media_block("video", bytearray(b"abcd"))
        self.assertEqual(source.emit.call_count, 4)

    def test_audio_samples_remain_contiguous_despite_read_jitter(self):
        publisher = Publisher.__new__(Publisher)
        publisher.media_lock = threading.Lock()
        publisher.media_epoch_ns = 0
        publisher.last_pts = {}
        publisher.audio_next_pts = None
        publisher.args = argparse.Namespace(fps_num=60, fps_den=1)
        publisher.pipeline = Mock()
        source = publisher.pipeline.get_by_name.return_value
        source.emit.return_value = "ok"
        gst = Mock()
        gst.SECOND = 1_000_000_000
        gst.CLOCK_TIME_NONE = -1
        gst.FlowReturn.OK = "ok"
        gst.Buffer.new_allocate.side_effect = lambda *args: Mock()
        namespace.update(Gst=gst, time=time, sys=Mock())
        arrivals = [20, 43, 55, 55, 104, 120, 320, 341]
        with patch("time.monotonic_ns", side_effect=[value * 1_000_000 for value in arrivals]):
            for _ in arrivals:
                publisher.push_media_block("audio", bytearray(3840))
        buffers = [call.args[1] for call in source.emit.call_args_list]
        self.assertEqual([buffer.pts // 1_000_000 for buffer in buffers], [20, 40, 60, 80, 100, 120, 320, 340])
        self.assertTrue(all(buffer.duration == 20_000_000 for buffer in buffers))
        self.assertEqual([index for index, buffer in enumerate(buffers) if buffer.set_flags.called], [0, 6])


class EncoderTests(unittest.TestCase):
    def test_real_encoders_with_bursts_and_peer_rebuild(self):
        try:
            import gi
        except ImportError:
            self.skipTest("Linux GStreamer bindings required")
        spec = importlib.util.spec_from_file_location("raw_publisher", SCRIPT)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        Gst = module.Gst
        Gst.init(None)
        # Exercise the exact media branches without requiring a browser/ICE.
        template = module.PIPELINE_TEMPLATE.replace(
            "webrtcbin name=webrtc bundle-policy=max-bundle latency=0", ""
        ).replace("! webrtc.", "! fakesink name=video_sink sync=false signal-handoffs=true", 1).replace(
            "! webrtc.", "! fakesink name=audio_sink sync=false signal-handoffs=true", 1
        )
        for attempt in range(2):
            publisher = module.RawSessionPublisher.__new__(module.RawSessionPublisher)
            publisher.media_lock = threading.Lock()
            publisher.last_pts = {}
            publisher.audio_next_pts = None
            publisher.args = argparse.Namespace(fps_num=60000, fps_den=1000)
            publisher.pipeline = Gst.parse_launch(template.format(
                width=304, height=224, fps_num=60000, fps_den=1000,
                video_bitrate_bps=1400000, audio_bitrate_bps=64000,
                keyframe_distance=15,
            ))
            counts = {"video": 0, "audio": 0}
            for label in ["video", "audio"]:
                sink = publisher.pipeline.get_by_name(f"{label}_sink")
                sink.connect("handoff", lambda *_args, key=label: counts.__setitem__(key, counts[key] + 1))
            publisher.pipeline.set_state(Gst.State.PLAYING)
            publisher.media_epoch_ns = time.monotonic_ns()
            bus = publisher.pipeline.get_bus()
            try:
                for frame in range(180):
                    publisher.push_media_block("video", bytearray([frame % 256, 0, 0, 0]) * (304 * 224))
                    # Deliver exactly 20 ms blocks on a 48 kHz sample timeline;
                    # two adjacent pushes simulate a bursty FIFO read.
                    if frame % 6 in (0, 2, 3, 4, 5):
                        publisher.push_media_block("audio", bytearray(3840))
                    # Burst a pair of frames, then recover nominal pacing.
                    time.sleep((0 if frame % 7 == 0 else 2 if frame % 7 == 1 else 1) / 60)
                    error = bus.pop_filtered(Gst.MessageType.ERROR)
                    self.assertIsNone(error, error.parse_error() if error else "")
                time.sleep(0.1)
                self.assertGreater(counts["video"], 30)
                self.assertGreater(counts["audio"], 30)
                error = bus.pop_filtered(Gst.MessageType.ERROR)
                self.assertIsNone(error, error.parse_error() if error else "")
            finally:
                publisher.pipeline.set_state(Gst.State.NULL)


if __name__ == "__main__":
    unittest.main()
