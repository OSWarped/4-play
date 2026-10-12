#!/usr/bin/env python3
"""
Plan a GStreamer WebRTC pipeline for 4-Play raw MAME media.

This is intentionally a planner/checker, not the live runtime integration.
It documents the exact raw-video/raw-audio assumptions needed before we wire
session-runtime media into a per-session WebRTC publisher.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys


REQUIRED_ELEMENTS = [
    "webrtcbin",
    "filesrc",
    "rawvideoparse",
    "rawaudioparse",
    "videoconvert",
    "audioconvert",
    "audioresample",
    "vp8enc",
    "rtpvp8pay",
    "opusenc",
    "rtpopuspay",
]


def gst_element_available(element: str) -> bool:
    return (
        subprocess.run(
            ["gst-inspect-1.0", element],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        ).returncode
        == 0
    )


def check_prerequisites() -> int:
    if shutil.which("gst-inspect-1.0") is None:
        print("gst-inspect-1.0 missing", file=sys.stderr)
        return 1

    missing = [element for element in REQUIRED_ELEMENTS if not gst_element_available(element)]
    for element in REQUIRED_ELEMENTS:
        state = "present" if element not in missing else "missing"
        print(f"{element:<16} {state}")
    return 1 if missing else 0


def build_pipeline(args: argparse.Namespace) -> str:
    return "\n".join(
        [
            "webrtcbin name=webrtc bundle-policy=max-bundle latency=0",
            f"filesrc location={quote_path(args.video_path)} do-timestamp=true",
            (
                "  ! rawvideoparse "
                f"format=bgrx width={args.width} height={args.height} "
                f"framerate={args.fps_num}/{args.fps_den}"
            ),
            "  ! queue leaky=downstream max-size-buffers=2 max-size-time=0 max-size-bytes=0",
            "  ! videoconvert",
            f"  ! vp8enc deadline=1 cpu-used=8 target-bitrate={args.video_bitrate_kbps * 1000} keyframe-max-dist={args.keyframe_distance}",
            "  ! rtpvp8pay pt=96",
            "  ! application/x-rtp,media=video,encoding-name=VP8,payload=96,clock-rate=90000",
            "  ! webrtc.",
            f"filesrc location={quote_path(args.audio_path)} do-timestamp=true",
            (
                "  ! rawaudioparse use-sink-caps=false format=pcm "
                f"pcm-format=s16le sample-rate={args.audio_rate} "
                f"num-channels={args.audio_channels}"
            ),
            "  ! queue leaky=downstream max-size-buffers=4 max-size-time=0 max-size-bytes=0",
            "  ! audioconvert",
            "  ! audioresample",
            f"  ! opusenc audio-type=restricted-lowdelay bitrate={args.audio_bitrate_bps}",
            f"  ! rtpopuspay pt={args.audio_payload_type}",
            f"  ! application/x-rtp,media=audio,encoding-name=OPUS,payload={args.audio_payload_type},clock-rate=48000",
            "  ! webrtc.",
        ]
    )


def quote_path(path: str) -> str:
    if any(character.isspace() for character in path):
        return repr(path)
    return path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Plan/check the future WebRTC raw MAME media pipeline.",
    )
    parser.add_argument("--video-path", default="/tmp/4play/session/video.raw")
    parser.add_argument("--audio-path", default="/tmp/4play/session/audio.pcm")
    parser.add_argument("--width", type=int, default=320)
    parser.add_argument("--height", type=int, default=224)
    parser.add_argument("--fps-num", type=int, default=60)
    parser.add_argument("--fps-den", type=int, default=1)
    parser.add_argument("--audio-rate", type=int, default=48000)
    parser.add_argument("--audio-channels", type=int, default=2)
    parser.add_argument("--video-bitrate-kbps", type=int, default=1400)
    parser.add_argument("--audio-bitrate-bps", type=int, default=64000)
    parser.add_argument("--audio-payload-type", type=int, default=111)
    parser.add_argument("--keyframe-distance", type=int, default=15)
    parser.add_argument(
        "--check-prereqs",
        action="store_true",
        help="Check required GStreamer elements and exit.",
    )
    parser.add_argument(
        "--print-pipeline",
        action="store_true",
        help="Print the planned gst-launch-style pipeline.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.check_prereqs:
        return check_prerequisites()
    if args.print_pipeline:
        print(build_pipeline(args))
        return 0
    print("Nothing to do. Use --check-prereqs or --print-pipeline.", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
