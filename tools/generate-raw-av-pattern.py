#!/usr/bin/env python3
"""
Generate 4-Play-shaped raw video/audio for throwaway pipeline tests.

Video: BGRA frames, matching MAME `-rawvideowrite` consumption assumptions.
Audio: signed 16-bit little-endian stereo PCM at 48 kHz.

The output paths may be regular files or named pipes/FIFOs. With FIFOs, start
the reader first or this script will block while opening the path for writing.
"""

from __future__ import annotations

import argparse
import math
import struct
import time
from pathlib import Path


def video_frame(width: int, height: int, frame_index: int) -> bytes:
    data = bytearray(width * height * 4)
    ball_x = frame_index * 3 % max(1, width)
    ball_y = frame_index * 2 % max(1, height)
    radius_squared = max(8, min(width, height) // 10) ** 2

    offset = 0
    for y in range(height):
        for x in range(width):
            stripe = ((x // 16) + (y // 16) + (frame_index // 4)) % 2
            blue = 42 if stripe else 8
            green = (x * 255) // max(1, width - 1)
            red = (y * 255) // max(1, height - 1)
            if (x - ball_x) ** 2 + (y - ball_y) ** 2 <= radius_squared:
                blue, green, red = 255, 255, 255
            data[offset : offset + 4] = bytes((blue, green, red, 255))
            offset += 4
    return bytes(data)


def audio_block(
    sample_rate: int,
    channels: int,
    block_samples: int,
    start_sample: int,
    frequency_hz: float,
) -> bytes:
    amplitude = 0.25 * 32767
    samples = bytearray(block_samples * channels * 2)
    offset = 0
    for sample_index in range(block_samples):
        phase = 2.0 * math.pi * frequency_hz * (start_sample + sample_index) / sample_rate
        value = int(math.sin(phase) * amplitude)
        packed = struct.pack("<h", value)
        for _channel in range(channels):
            samples[offset : offset + 2] = packed
            offset += 2
    return bytes(samples)


def run(args: argparse.Namespace) -> None:
    frame_interval = 1.0 / args.fps
    total_frames = int(args.duration_seconds * args.fps)
    block_samples = args.audio_rate * args.audio_block_ms // 1000
    audio_sample_cursor = 0

    Path(args.video_path).parent.mkdir(parents=True, exist_ok=True)
    Path(args.audio_path).parent.mkdir(parents=True, exist_ok=True)

    print(
        "Writing raw pattern: "
        f"video={args.video_path} {args.width}x{args.height}@{args.fps}fps "
        f"audio={args.audio_path} {args.audio_rate}Hz/{args.audio_channels}ch "
        f"duration={args.duration_seconds}s",
        flush=True,
    )

    with open(args.video_path, "wb", buffering=0) as video, open(
        args.audio_path,
        "wb",
        buffering=0,
    ) as audio:
        next_frame_at = time.monotonic()
        for frame_index in range(total_frames):
            video.write(video_frame(args.width, args.height, frame_index))
            audio.write(
                audio_block(
                    args.audio_rate,
                    args.audio_channels,
                    block_samples,
                    audio_sample_cursor,
                    args.audio_frequency_hz,
                )
            )
            audio_sample_cursor += block_samples
            next_frame_at += frame_interval
            delay = next_frame_at - time.monotonic()
            if delay > 0:
                time.sleep(delay)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate BGRA video and S16LE stereo PCM for raw media tests.",
    )
    parser.add_argument("--video-path", required=True)
    parser.add_argument("--audio-path", required=True)
    parser.add_argument("--width", type=int, default=320)
    parser.add_argument("--height", type=int, default=224)
    parser.add_argument("--fps", type=int, default=60)
    parser.add_argument("--duration-seconds", type=float, default=10.0)
    parser.add_argument("--audio-rate", type=int, default=48000)
    parser.add_argument("--audio-channels", type=int, default=2)
    parser.add_argument("--audio-block-ms", type=int, default=20)
    parser.add_argument("--audio-frequency-hz", type=float, default=880.0)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.width <= 0 or args.height <= 0 or args.fps <= 0:
        raise SystemExit("width, height, and fps must be greater than zero")
    if args.audio_rate <= 0 or args.audio_channels <= 0 or args.audio_block_ms <= 0:
        raise SystemExit("audio rate, channels, and block duration must be greater than zero")
    if args.duration_seconds <= 0:
        raise SystemExit("duration must be greater than zero")
    run(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
