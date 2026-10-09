#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
environment_directory="/etc/4play"
environment_file="${environment_directory}/4play.env"
control_plane_unit="/etc/systemd/system/4play-control-plane.service"
host_agent_unit="/etc/systemd/system/4play-runtime-host-agent.service"

if [[ "${EUID}" -ne 0 ]]; then
    printf 'Run this installer with sudo:\n'
    printf '  sudo %s\n' "$0"
    exit 1
fi

install -d -m 0750 "$environment_directory"
install -d -m 0755 /var/lib/4play

if [[ ! -f "$environment_file" ]]; then
    install -m 0640 "$repository_root/deploy/systemd/4play.env.example" "$environment_file"
    printf 'Created %s from the example template.\n' "$environment_file"
    printf 'Review tokens, paths, and network addresses before enabling services.\n'
else
    printf 'Keeping existing %s.\n' "$environment_file"
fi

install -m 0644 "$repository_root/deploy/systemd/4play-control-plane.service" "$control_plane_unit"
install -m 0644 "$repository_root/deploy/systemd/4play-runtime-host-agent.service" "$host_agent_unit"

systemctl daemon-reload

printf '\nInstalled systemd unit files:\n'
printf '  %s\n' "$control_plane_unit"
printf '  %s\n' "$host_agent_unit"
printf '\nNext commands after reviewing %s:\n' "$environment_file"
printf '  sudo systemctl enable --now 4play-control-plane.service\n'
printf '  sudo systemctl enable --now 4play-runtime-host-agent.service\n'
printf '  systemctl status 4play-control-plane.service 4play-runtime-host-agent.service\n'

