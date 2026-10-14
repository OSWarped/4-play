"""Bounded per-session peer ownership, independent of GStreamer bindings."""
import threading
import uuid

from webrtc_browser_input import BrowserInputRelay


class PeerCapacityError(RuntimeError):
    pass


class PeerPool:
    def __init__(self, factory, max_peers=8):
        if not 1 <= max_peers <= 32:
            raise ValueError("max peers must be between 1 and 32")
        self.factory = factory
        self.max_peers = max_peers
        self.lock = threading.RLock()
        self.peers = {}
        self.relays = {}

    def create(self, authorization, viewer_key=None):
        with self.lock:
            # A reconnect may replace its own slot, never another player/viewer.
            key = f"player:{authorization.player}" if authorization else f"viewer:{viewer_key or uuid.uuid4()}"
            # Promotion relinquishes only the server-identified spectator view.
            if authorization and viewer_key:
                viewer = self.peers.pop(f"viewer:{viewer_key}", None)
                if viewer:
                    viewer.close()
            if key not in self.peers and len(self.peers) >= self.max_peers:
                raise PeerCapacityError("session browser peer limit reached")
            old = self.peers.pop(key, None)
            if old:
                old.close()
            relay = None
            if authorization:
                relay = self.relays.get(authorization.player)
                if relay is None:
                    relay = self.relays[authorization.player] = BrowserInputRelay()
            peer = self.factory(relay)
            self.peers[key] = peer
            return key, peer

    def remove(self, key, peer):
        with self.lock:
            if self.peers.get(key) is peer:
                del self.peers[key]
                peer.close()

    def snapshot(self):
        with self.lock:
            return list(self.peers.items())

    def renew(self, authorization):
        if authorization is None:
            return False
        with self.lock:
            relay = self.relays.get(authorization.player)
            return relay.renew(authorization) if relay else False

    def close(self):
        with self.lock:
            for peer in self.peers.values():
                peer.close()
            self.peers.clear()
            for relay in self.relays.values():
                relay.close()
            self.relays.clear()
