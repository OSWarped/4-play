#!/usr/bin/env python3
"""Build the same cached WebP card images served on demand by the control plane."""

import argparse
import os
from pathlib import Path
import subprocess
import sys
import uuid

SOURCE_EXTENSIONS = {".gif", ".jpg", ".jpeg", ".png", ".webp"}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--asset-root", type=Path, default=Path("data/assets"))
    args = parser.parse_args()
    root = args.asset_root.resolve(strict=True)
    thumbnail_root = root / "thumbnails"
    sources = sorted(
        path for path in root.rglob("*")
        if path.is_file()
        and thumbnail_root not in path.parents
        and path.suffix.lower() in SOURCE_EXTENSIONS
        and path.stat().st_size <= 10 * 1024 * 1024
    )
    created = skipped = failed = 0
    for source in sources:
        relative = source.relative_to(root)
        destination = thumbnail_root / relative.parent / f"{relative.name}.webp"
        if destination.is_file() and destination.stat().st_size > 0 \
                and destination.stat().st_mtime_ns >= source.stat().st_mtime_ns:
            skipped += 1
            continue
        destination.parent.mkdir(parents=True, exist_ok=True)
        temporary = destination.parent / f".thumbnail-{uuid.uuid4()}.webp"
        result = subprocess.run(
            ["ffmpeg", "-v", "error", "-nostdin", "-y", "-i", str(source),
             "-vf", "scale=480:360:force_original_aspect_ratio=decrease",
             "-frames:v", "1", "-c:v", "libwebp", "-quality", "72",
             "-compression_level", "6", str(temporary)],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=60,
        )
        if result.returncode == 0 and temporary.is_file() and temporary.stat().st_size > 0:
            os.replace(temporary, destination)
            created += 1
        else:
            temporary.unlink(missing_ok=True)
            failed += 1
            print(f"thumbnail failed: {relative}", file=sys.stderr)
    print(f"artwork thumbnails: created={created} current={skipped} failed={failed}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
