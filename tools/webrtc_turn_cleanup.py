"""Release remote TURN allocations before destroying a WAN peer's ICE agent.

Closing never blocks the peer pool or media feeder. GLib owns the bounded async
operation, retaining the pipeline until libnice completes (or a timeout). LAN
peers do not use this helper. No credentials or SDP are logged.
"""
import sys
import threading
import time

_pending = set()
_lock = threading.Lock()

# Register the GI class before GStreamer first wraps a NiceAgent. Loading its
# typelib after that produces a cached generic wrapper without native methods.
# Absence is tolerated for LAN-only installations.
try:
    import gi
    gi.require_version("Nice", "0.1")
    from gi.repository import Nice as _nice
except (ImportError, ValueError):
    _nice = None


def require_cleanup_binding():
    if _nice is None or not callable(getattr(_nice.Agent, "close_async", None)):
        raise RuntimeError("WAN relay cleanup requires Debian package gir1.2-nice-0.1")
    return _nice


class TurnCleanup:
    def __init__(self, pipeline, webrtc, gst, glib, nice=None):
        self.pipeline, self.webrtc = pipeline, webrtc
        self.gst, self.glib = gst, glib
        self.nice = nice if nice is not None else require_cleanup_binding()
        self.done = threading.Event()
        self.released = False
        self.timeout_id = None
        self.agent = None
        with _lock:
            _pending.add(self)
        # Always call libnice in the running default GLib context, not a peer
        # maintenance/HTTP thread whose context is never iterated.
        glib.idle_add(self.start)

    def start(self):
        if self.done.is_set():
            return False
        self.timeout_id = self.glib.timeout_add(5000, self.timed_out)
        try:
            ice = self.webrtc.get_property("ice-agent")
            # Same GstInitiallyUnowned ownership workaround as port setup. A
            # receiver may not have previously wrapped this default ICE object.
            if getattr(ice, "ref_count", 2) == 1:
                ice._ref()
            self.agent = ice.get_property("agent")
            self.nice.Agent.close_async(self.agent, self.completed, None)
        except Exception:  # Native failures must still release the local pipeline.
            print("WAN TURN cleanup could not start; remote release unconfirmed", file=sys.stderr, flush=True)
            self.finish(False)
        return False

    def completed(self, *_args):
        self.finish(True)

    def timed_out(self):
        self.timeout_id = None
        print("WAN TURN cleanup timed out; remote release unconfirmed", file=sys.stderr, flush=True)
        self.finish(False)
        return False

    def finish(self, released):
        # Both the timeout and native completion execute in the same context.
        if self.done.is_set():
            return
        if self.timeout_id is not None:
            self.glib.source_remove(self.timeout_id)
            self.timeout_id = None
        self.released = released
        try:
            self.pipeline.set_state(self.gst.State.NULL)
        finally:
            self.pipeline = self.webrtc = self.agent = None
            self.done.set()
            with _lock:
                _pending.discard(self)


def drain_turn_cleanup(glib, timeout=6):
    """After the main loop stops, give shutdown callbacks a bounded grace period."""
    context = glib.MainContext.default()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        with _lock:
            if not _pending:
                return True
        context.iteration(False)
        time.sleep(.005)
    return False
