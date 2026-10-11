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
echo
check_gst_element webrtcbin
check_gst_element x264enc
check_gst_element avenc_aac
check_gst_element audiotestsrc
check_gst_element videotestsrc
echo

if [ "$missing" -eq 0 ]; then
  echo "Browser-native media spike prerequisites look present."
else
  echo "One or more browser-native media spike prerequisites are missing."
  echo "On Debian, start by investigating: gstreamer1.0-tools gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly"
fi

exit "$missing"
