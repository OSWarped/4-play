"""Bounded WAN replacement grace; never sleep under the peer-pool lock.

Nice close completion acknowledges TURN deallocation but coturn may defer the
native deletion to its next timer tick. This bounded grace reduces immediate
replacement pressure; it is not evidence that arbitrary relay capacity is free.
LAN replacements and unrelated media peers are left alone.
"""
import time


def release_previous_wan_peer(pool, authorization, viewer_key, pause=time.sleep):
    key = f"player:{authorization.player}" if authorization else f"viewer:{viewer_key}"
    old = next((peer for candidate, peer in pool.snapshot() if candidate == key), None)
    if old is None or not getattr(old, "wan_mode", False):
        return
    pool.remove(key, old)
    cleanup = getattr(old, "turn_cleanup", None)
    if cleanup is None or not cleanup.done.wait(5) or not cleanup.released:
        raise RuntimeError("Previous WAN relay release is unconfirmed; retry media after cleanup")
    pause(1.25)
