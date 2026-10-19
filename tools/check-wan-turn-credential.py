"""Local deployment credential check only; no network, GI or secret output."""
import os
from webrtc_wan import WanIce


def main():
    if os.environ.get("FOURPLAY_WAN_TURN_CREDENTIAL") != "4play-turn.conf" or "FOURPLAY_WAN_TURN_SECRET" in os.environ:
        raise SystemExit("Expected systemd TURN credential without plaintext environment secret")
    WanIce.from_environment()
    print("PASS: Python TURN systemd credential validated; no secret printed or network opened")


if __name__ == "__main__": main()
