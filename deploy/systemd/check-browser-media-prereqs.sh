#!/usr/bin/env bash
set -euo pipefail

missing=0

check_command() {
  local name="$1"
  if command -v "$name" >/dev/null 2>&1; then
    printf '%-24s %s\n' "$name" "present"
  else
    printf '%-24s %s\n' "$name" "missing"
    missing=1
  fi
}

check_gst_element() {
  local element="$1"
  if ! command -v gst-inspect-1.0 >/dev/null 2>&1; then
    printf '%-24s %s\n' "$element" "unknown (gst-inspect-1.0 missing)"
    missing=1
    return
  fi
  if gst-inspect-1.0 "$element" >/dev/null 2>&1; then
    printf '%-24s %s\n' "$element" "present"
  else
    printf '%-24s %s\n' "$element" "missing"
    missing=1
  fi
}

echo "4-Play browser-native media prerequisite check"
echo
check_command gst-launch-1.0
check_command gst-inspect-1.0
check_command python3
echo
check_gst_element webrtcbin
check_gst_element x264enc
check_gst_element avenc_aac
check_gst_element opusenc
check_gst_element rtph264pay
check_gst_element rtpopuspay
check_gst_element h264parse
check_gst_element audiotestsrc
check_gst_element videotestsrc
echo

if command -v python3 >/dev/null 2>&1; then
  if python3 - <<'PY' >/dev/null 2>&1
import gi
gi.require_version("Gst", "1.0")
gi.require_version("GstSdp", "1.0")
gi.require_version("GstWebRTC", "1.0")
from gi.repository import Gst, GstSdp, GstWebRTC
PY
  then
    printf '%-24s %s\n' "python3 gi GstWebRTC" "present"
  else
    printf '%-24s %s\n' "python3 gi GstWebRTC" "missing"
    missing=1
  fi
else
  printf '%-24s %s\n' "python3 gi GstWebRTC" "unknown (python3 missing)"
fi
echo

if [ "$missing" -eq 0 ]; then
  echo "Browser-native media spike prerequisites look present."
else
  echo "One or more browser-native media spike prerequisites are missing."
  echo "On Debian, start by investigating: python3-gi gir1.2-gstreamer-1.0 gir1.2-gst-plugins-bad-1.0 gstreamer1.0-tools gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav"
fi

exit "$missing"
