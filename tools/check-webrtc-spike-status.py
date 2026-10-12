#!/usr/bin/env python3
"""
Check the non-browser pieces of the 4-Play WebRTC browser-media spike.

This intentionally does not create a PeerConnection or launch GStreamer media.
It verifies that the control-plane experimental status endpoint is reachable,
that the optional test-pattern proxy is configured when requested, and that the
standalone spike publisher health endpoint is reachable when supplied.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from typing import Any


def main() -> int:
    args = parse_args()
    failures: list[str] = []

    control_plane_url = args.control_plane_url.rstrip("/")
    token = args.api_token or os.environ.get("FOURPLAY_SEAT_API_TOKEN", "")
    status_url = f"{control_plane_url}/api/v1/experimental/webrtc/status"

    print("4-Play WebRTC spike status check")
    print()
    print(f"control plane: {control_plane_url}")

    try:
        health = read_text(f"{control_plane_url}/health")
        print(f"control-plane health: {health.strip()}")
    except RuntimeError as error:
        failures.append(f"control-plane health failed: {error}")

    if not token:
        failures.append("missing seat API token; set FOURPLAY_SEAT_API_TOKEN or pass --api-token")
    else:
        try:
            status = read_json(status_url, token)
            print_status(status)
            if args.require_test_pattern_proxy and not status.get("test_pattern_proxy_configured"):
                failures.append("test-pattern proxy is not configured")
            if args.require_session_publisher and not status.get("session_publisher_available"):
                failures.append("no runtime host is advertising WebRTC session publishers")
        except RuntimeError as error:
            failures.append(f"WebRTC status failed: {error}")

    if args.publisher_health_url:
        try:
            publisher_health = read_text(args.publisher_health_url)
            print(f"publisher health: {publisher_health.strip()}")
        except RuntimeError as error:
            failures.append(f"publisher health failed: {error}")

    if failures:
        print()
        print("WebRTC spike status check failed:")
        for failure in failures:
            print(f"  - {failure}")
        return 1

    print()
    print("WebRTC spike status check passed.")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--control-plane-url",
        default=os.environ.get("FOURPLAY_CONTROL_PLANE_URL", "http://127.0.0.1:8080"),
        help="control-plane base URL (default: FOURPLAY_CONTROL_PLANE_URL or http://127.0.0.1:8080)",
    )
    parser.add_argument(
        "--api-token",
        default="",
        help="seat API token (default: FOURPLAY_SEAT_API_TOKEN)",
    )
    parser.add_argument(
        "--publisher-health-url",
        default="",
        help="optional standalone test-pattern publisher health URL, for example http://127.0.0.1:18080/health",
    )
    parser.add_argument(
        "--require-test-pattern-proxy",
        action="store_true",
        help="fail unless the control plane reports the test-pattern publisher proxy is configured",
    )
    parser.add_argument(
        "--require-session-publisher",
        action="store_true",
        help="fail unless at least one runtime host advertises WebRTC session publishers",
    )
    return parser.parse_args()


def read_text(url: str) -> str:
    request = urllib.request.Request(url)
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            body = response.read().decode("utf-8", errors="replace")
    except (OSError, urllib.error.HTTPError, urllib.error.URLError) as error:
        raise RuntimeError(str(error)) from error
    return body


def read_json(url: str, token: str) -> dict[str, Any]:
    request = urllib.request.Request(url, headers={"Authorization": f"Bearer {token}"})
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            body = response.read().decode("utf-8", errors="replace")
    except (OSError, urllib.error.HTTPError, urllib.error.URLError) as error:
        raise RuntimeError(str(error)) from error
    try:
        parsed = json.loads(body)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"invalid JSON response: {error}") from error
    if not isinstance(parsed, dict):
        raise RuntimeError("expected JSON object response")
    return parsed


def print_status(status: dict[str, Any]) -> None:
    hosts = status.get("web_rtc_runtime_hosts") or []
    if not isinstance(hosts, list):
        hosts = []
    print(f"test-pattern proxy: {configured(status.get('test_pattern_proxy_configured'))}")
    print(f"session offer endpoint: {configured(status.get('session_offer_endpoint_available'))}")
    print(f"session publisher: {configured(status.get('session_publisher_available'))}")
    print(f"WebRTC runtime hosts: {', '.join(map(str, hosts)) if hosts else 'none'}")
    print(f"test-pattern offer path: {status.get('test_pattern_offer_path') or 'unknown'}")
    print(f"session offer path: {status.get('session_offer_path_template') or 'unknown'}")


def configured(value: object) -> str:
    return "configured" if value else "not configured"


if __name__ == "__main__":
    sys.exit(main())
