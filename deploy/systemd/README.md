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

Then enable and start:

```bash
sudo systemctl enable --now 4play-control-plane.service
sudo systemctl enable --now 4play-runtime-host-agent.service
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

## Firewall

Allow the reference control and data-plane ports from trusted LAN clients:

```bash
sudo ufw allow from 192.168.20.0/24 to any port 8080 proto tcp
sudo ufw allow from 192.168.20.0/24 to any port 41000:41099 proto udp
sudo ufw allow from 192.168.20.0/24 to any port 42000:42099 proto udp
```

If the host does not use `ufw`, apply equivalent rules for the active firewall.

