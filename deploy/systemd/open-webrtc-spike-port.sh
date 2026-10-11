#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  deploy/systemd/open-webrtc-spike-port.sh --seat-ip <ip> [--port <port>] [--apply]

Purpose:
  Opens the temporary WebRTC spike HTTP signaling port from one seat/client IP.
  Without --apply, this prints the command only.

Examples:
  deploy/systemd/open-webrtc-spike-port.sh --seat-ip 192.168.20.10
  deploy/systemd/open-webrtc-spike-port.sh --seat-ip 192.168.20.10 --apply
  deploy/systemd/open-webrtc-spike-port.sh --seat-ip 192.168.20.10 --port 18080 --apply
USAGE
}

seat_ip=""
port="18080"
apply="0"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --seat-ip)
      seat_ip="${2:-}"
      shift 2
      ;;
    --port)
      port="${2:-}"
      shift 2
      ;;
    --apply)
      apply="1"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ -z "$seat_ip" ]; then
  echo "missing --seat-ip <ip>" >&2
  usage >&2
  exit 2
fi

if ! [[ "$port" =~ ^[0-9]+$ ]] || [ "$port" -lt 1 ] || [ "$port" -gt 65535 ]; then
  echo "invalid --port: $port" >&2
  exit 2
fi

echo "Temporary WebRTC spike port rule:"
echo "  seat/client IP: $seat_ip"
echo "  TCP port:       $port"
echo

if command -v ufw >/dev/null 2>&1 && sudo ufw status >/dev/null 2>&1; then
  echo "ufw appears available. Equivalent persistent-ish command:"
  echo "  sudo ufw allow from $seat_ip to any port $port proto tcp"
  echo
fi

echo "iptables command:"
echo "  sudo iptables -I INPUT -p tcp -s $seat_ip --dport $port -j ACCEPT"
echo

if [ "$apply" != "1" ]; then
  echo "Dry run only. Re-run with --apply to insert the iptables rule."
  exit 0
fi

sudo iptables -I INPUT -p tcp -s "$seat_ip" --dport "$port" -j ACCEPT
echo "Applied temporary iptables rule."
echo
echo "Verify from the seat/client:"
echo "  Invoke-WebRequest http://$(hostname -I | awk '{print $1}'):$port/health"
