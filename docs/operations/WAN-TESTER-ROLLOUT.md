# Browser-only WAN tester rollout

Target: `https://arcade.blakemilam.com/client-v4`. Testers use a normal browser,
without installing a VPN or entering service API tokens.

Status: DNS added by the owner and verified as `45.24.46.155`. HTTPS staging
activated successfully by the owner; certificate issued with automatic renewal.
The owner verified the intended staging response. No playable public endpoint,
new firewall/router rule, or public enrollment flow has been activated.

Private invitation storage/operator CLI and an opt-in browser gate with experimental
WAN ICE are implemented; see [browser access](../testing/WAN-BROWSER-ACCESS.md)
for completed pieces, activation blockers, and the next masked coturn installation
step. They are not activated on the reference host. Keep staging 503 until all
gates pass.

Operator checkpoint (2026-10-21): protected TURN credentials installed, both
application services restarted, and LAN `/client-v4` gameplay reconfirmed. Next:
[private enrollment and token rotation walkthrough](WAN-ENROLLMENT-SETUP.md).
The new helper stages configuration only; public activation is still separate.

## Verified starting point (2026-10-18)

- Linux host `192.168.20.68`: nginx, control plane, and runtime host agent active.
- Existing nginx sites: `blakemilam.com` and `blakdusttriviahost.com`; preserve both.
- Certbot is installed; `turnserver` was not found on PATH.
- Initial DNS lookup returned name not found; after the owner added the A record,
  lookup resolved to `45.24.46.155`.
- Browser seat enrollment validates the actual transport peer against LAN CIDRs;
  forwarded headers are intentionally not trusted.
- Browser peers currently use `iceServers: []`. Publisher candidate validation
  accepts bounded UDP host candidates only. Neither is internet-ready.

## Rollout gates

1. DNS: owner adds an A record named `arcade` in `blakemilam.com` pointing to
   the current public WAN IPv4. The previously supplied address was
   `45.24.46.155`; confirm it is still current first. Use DNS-only mode for the
   initial experiment, not a CDN proxy. Do not add AAAA without a tested IPv6 path.
2. Protected HTTPS: prepare a separate nginx virtual host, obtain its certificate,
   test nginx configuration, and reload without changing existing sites. Do not
   expose a usable public game endpoint before tester access is enforced.
3. Tester enrollment: add revocable, expiring invitations with bounded seat/session
   capacity and request limits. Keep shared service tokens server-side. Preserve
   LAN enrollment behavior; do not approve all networks to bypass its checks.
4. Trusted proxy integration: explicitly trust only the configured local proxy;
   it must replace client-IP headers. Validate public Host/Origin, Secure cookies,
   cross-site requests, and direct-backend header spoofing. Do not treat every
   loopback request as a successfully authenticated tester.
5. Internet media: configure ICE on browser and publisher with STUN and authenticated
   TURN fallback, using short-lived relay credentials. Introduce an explicit WAN
   profile while preserving the existing bounded LAN profile. Do not rewrite browser
   mDNS candidates to a public TCP peer address: NAT ports may differ as well.
6. Network activation: document exact ports for the selected TURN deployment before
   opening them. Check public IP/NAT, relay advertised address, UDP relay range,
   TLS certificate, credential expiry, relay limits, and relay permission rules
   that prevent access to unrelated internal services. Never run an open relay.
7. External validation: test over cellular or a genuinely remote network, not only
   LAN NAT hairpin. Verify direct ICE and deliberately forced TURN, then normal
   automatic selection. Confirm leave/reconnect, multiple seats, and cleanup.

Keep `/admin`, raw control-plane APIs, internal publisher HTTP, and legacy input/
media ports private. Public nginx routing must allow only required client assets
and scoped client endpoints, rather than proxying every backend route. Rotate
development tokens before public activation. Avoid invitation secrets in logs.

## Owner action: DNS

At the DNS provider for `blakemilam.com`:

| Type | Name | Value |
| --- | --- | --- |
| A | arcade | Current public WAN IPv4 (confirm the previously supplied address) |

Verify in Windows PowerShell:

```powershell
Resolve-DnsName arcade.blakemilam.com -Type A
```

The existing TCP 80/443 router forwards can serve this hostname if they still
terminate on this Linux host. Do not forward 8080 or publisher 430xx HTTP ports.
DNS resolution is not evidence that public gameplay is enabled.

## Owner action: isolated HTTPS staging

Run on Linux from the updated repository. The tool only manages its own exact
staging config, validates nginx before/after changes, rolls back failed validation
or reload, and never proxies the app. It does not change service environment,
firewall, router rules, or either existing site's configuration.

```bash
cd ~/src/4-play
python3 tools/test-public-edge.py
sudo python3 deploy/nginx/prepare-public-edge.py --apply
sudo certbot certonly --webroot -w /var/www/4play-acme \
  --cert-name arcade.blakemilam.com -d arcade.blakemilam.com \
  --deploy-hook 'nginx -t && systemctl reload nginx'
sudo python3 deploy/nginx/prepare-public-edge.py --apply --tls
curl -i --resolve arcade.blakemilam.com:443:127.0.0.1 \
  https://arcade.blakemilam.com/client-v4
```

Run each command only after the previous one succeeds. Certbot may prompt for
email and agreement to its terms; do not paste credentials into chat. This uses
webroot issuance rather than letting Certbot modify other nginx sites. Existing
TCP 80 forwarding must work for certificate issuance. The deploy hook reloads
nginx after successful renewals; check `systemctl list-timers --all | grep certbot`
and validate renewal with `sudo certbot renew --cert-name arcade.blakemilam.com --dry-run`.

Expected result: valid HTTPS, **503** with `4-Play external testing is not ready
yet.` on `/client-v4`, `/admin`, and all application routes. This is intentional,
not a playable deployment. HTTP redirects to HTTPS after TLS setup; ACME challenge
paths remain available. Do not invite testers yet. The commands require the owner's
sudo password and do not require another router forward.

The staging tool refuses unknown/customized files. Once the real protected site
is installed, do not reuse this staging tool to manage it. If rollback is needed,
remove only `/etc/nginx/sites-enabled/4play-arcade-staging`, then `sudo nginx -t`
and `sudo systemctl reload nginx`; retain certificate and available config for
recovery. Never remove or disable the pre-existing sites.

## Acceptance checks

Private TURN protocol + decoded media confinement passed on the reference host
at `76468d8` and `de9d451`; this does not enable public access or establish internet NAT behavior.
The client now schedules fresh whole-peer credentials at 45 minutes for WAN only.
This can briefly interrupt media/input; real long-running browser acceptance is
still required. The updated replacement grace also passed the operator's confined
gate. Next prepare the separate tested-candidate bundle using
`stage-tested-turn-candidate.py --apply --accept-local-patch` with sudo; this
reruns the gate but installs/activates no service. Preserve the original baseline.
See [current validation evidence](../testing/WAN-BROWSER-ACCESS.md).

- Uninvited/expired/revoked testers cannot enroll or create media sessions.
- Admin and unscoped APIs remain inaccessible via the public hostname.
- Forged proxy headers cannot bypass enrollment or alter the trusted media peer.
- Existing LAN playback and both existing nginx sites still work.
- Browser reports selected candidate type, RTT, jitter/loss, and decode/render
  buffering where available. Never log private session or TURN credentials.
- One nearby tester first spectates, then plays TMNT and Killer Instinct for
  15–20 minutes. Check A/V sync, sustained input, combos, stalls, and delay growth.
- Record network, browser/device, direct-versus-relay route, server CPU, and upload
  bandwidth. RTT is not button-to-photon latency; do not promise LAN responsiveness.
- Stop/leave releases processes, grants, relay allocations, and capacity.

Rollback: disable only the new virtual host, revoke tester access, remove only
new WAN-specific forwarding/firewall rules, and restore the LAN transport profile.
Keep existing sites and LAN configuration intact.

## Protocol references

- [WebRTC ICE and peer connections](https://webrtc.org/getting-started/peer-connections)
- [WebRTC TURN servers](https://webrtc.org/getting-started/turn-server)
- [nginx reverse proxy](https://docs.nginx.com/nginx/admin-guide/web-server/reverse-proxy/)
