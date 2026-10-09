# Systemd Reference Server Setup

These files are the Phase 3 reference deployment templates for the Linux
runtime/control server.

## Install

From the server repository:

```bash
cd ~/src/4-play
sudo deploy/systemd/install-reference-server.sh
```

The installer:

- creates `/etc/4play/4play.env` from `4play.env.example` if it does not
  already exist
- installs the control-plane and runtime-host-agent unit files
- reloads systemd
- does not start services automatically

Review the environment file before starting services:

```bash
sudo nano /etc/4play/4play.env
```

The reference deployment should use the checked-in test catalog unless a
site-specific catalog has been created:

```text
FOURPLAY_CATALOG_PATH=/home/blake/src/4-play/catalog/test-catalog.json
```

Then enable and start:

```bash
sudo systemctl enable --now 4play-control-plane.service
sudo systemctl enable --now 4play-runtime-host-agent.service
```

For the reference headless MAME runtime, skip startup warnings and game-info
screens so games can begin producing raw media without local UI
acknowledgement:

```bash
sudo sed -i 's/^skip_warnings .*/skip_warnings             1/' /opt/4play/config/mame/ui.ini
sudo sed -i 's/^skip_gameinfo .*/skip_gameinfo             1/' /opt/4play/config/mame/mame.ini
sudo systemctl restart 4play-runtime-host-agent.service
```

Check status:

```bash
systemctl status 4play-control-plane.service 4play-runtime-host-agent.service
```

Run diagnostics:

```bash
cd ~/src/4-play
tools/phase-3-diagnostics.sh
```

If the service installation created a fresh control-plane database, restore the
reference presentation metadata and placeholder assets:

```bash
cd ~/src/4-play
FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin seed-known-metadata

FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin seed-placeholders \
  --asset-root /home/blake/src/4-play/data/assets \
  --update-metadata

FOURPLAY_CONTROL_PLANE_URL=http://127.0.0.1:8080 \
FOURPLAY_SEAT_API_TOKEN=phase-1c-seat-token-2026 \
./target/release/catalog-admin report
```

## Firewall

Generate the firewall plan from the same environment file used by systemd:

```bash
cd ~/src/4-play
FOURPLAY_ENV_FILE=/etc/4play/4play.env \
FOURPLAY_TRUSTED_SOURCE=192.168.20.0/24 \
tools/phase-3-firewall-plan.sh
```

For the default reference ranges, this prints:

```bash
sudo ufw allow from 192.168.20.0/24 to any port 8080 proto tcp
sudo ufw allow from 192.168.20.0/24 to any port 41000:41099 proto udp
sudo ufw allow from 192.168.20.0/24 to any port 42000:42099 proto udp
```

If the host does not use `ufw`, apply equivalent rules for the active firewall.
The planner also prints nftables-style equivalents.
