#!/usr/bin/env python3
"""Stage an isolated, fail-closed arcade hostname; never proxy the application.

Default: print the proposed config. --apply requires root on the reference Linux
host. Run initially without --tls, obtain the certificate with Certbot webroot,
then run with --tls. Existing unrelated sites are never edited.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

DOMAIN = "arcade.blakemilam.com"
MARKER = "# Managed by 4-Play prepare-public-edge.py; staging only.\n"
SITE = Path("/etc/nginx/sites-available/4play-arcade-staging")
ENABLED = Path("/etc/nginx/sites-enabled/4play-arcade-staging")
WEBROOT = Path("/var/www/4play-acme")
CERT = Path("/etc/letsencrypt/live") / DOMAIN


def render(tls=False):
    challenge = f"""    location ^~ /.well-known/acme-challenge/ {{
        root {WEBROOT.as_posix()};
        default_type text/plain;
        try_files $uri =404;
    }}
"""
    unavailable = """    location / {
        default_type text/plain;
        add_header Cache-Control "no-store" always;
        add_header Retry-After "3600" always;
        return 503 "4-Play external testing is not ready yet.\\n";
    }
"""
    http = (f"    location / {{ return 302 https://{DOMAIN}$request_uri; }}\n"
            if tls else unavailable)
    config = (MARKER + f"server {{\n    listen 80;\n    server_name {DOMAIN};\n"
              + challenge + http + "}\n")
    if tls:
        config += (f"server {{\n    listen 443 ssl;\n    server_name {DOMAIN};\n"
                   f"    ssl_certificate {CERT.as_posix()}/fullchain.pem;\n"
                   f"    ssl_certificate_key {CERT.as_posix()}/privkey.pem;\n"
                   "    ssl_protocols TLSv1.2 TLSv1.3;\n"
                   "    add_header X-Content-Type-Options nosniff always;\n"
                   + unavailable + "}\n")
    return config


def atomic_write(path, content):
    descriptor, name = tempfile.mkstemp(prefix=".4play-staging-", dir=path.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.chmod(0o644)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def install(config, site=SITE, enabled=ENABLED, run=subprocess.run):
    # Only exact outputs of this tool are eligible for replacement. A subsequent
    # application-facing config cannot accidentally be downgraded by rerunning it.
    if site.is_symlink():
        raise ValueError("Refusing a symlink at the staging configuration path")
    previous = site.read_bytes() if site.exists() else None
    if previous is not None and previous not in [render(False).encode(), render(True).encode()]:
        raise ValueError("Existing staging config differs; refusing to overwrite it")
    existed = enabled.is_symlink()
    if existed:
        if enabled.readlink() != site:
            raise ValueError("Enabled site points elsewhere; refusing to replace it")
    elif enabled.exists():
        raise ValueError("Enabled site is not the expected symlink")
    # Validate the pre-existing nginx configuration before touching our paths.
    run(["nginx", "-t"], check=True)
    try:
        atomic_write(site, config.encode())
        if not existed:
            enabled.symlink_to(site)
        run(["nginx", "-t"], check=True)
        run(["systemctl", "reload", "nginx"], check=True)
    except Exception:
        if not existed:
            enabled.unlink(missing_ok=True)
        if previous is None:
            site.unlink(missing_ok=True)
        else:
            atomic_write(site, previous)
        # Reload the restored, validated configuration too: a failed reload
        # command can leave ambiguity about which configuration is running.
        run(["nginx", "-t"], check=True)
        run(["systemctl", "reload", "nginx"], check=True)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--tls", action="store_true")
    args = parser.parse_args()
    config = render(args.tls)
    if not args.apply:
        print(config, end="")
        return
    if not hasattr(os, "geteuid") or os.geteuid() != 0:
        parser.error("--apply requires sudo on the Linux server")
    if not SITE.parent.is_dir() or not ENABLED.parent.is_dir():
        parser.error("Expected Debian nginx sites-available/sites-enabled directories")
    if args.tls and not all((CERT / name).is_file() for name in ["fullchain.pem", "privkey.pem"]):
        parser.error("Obtain the arcade certificate with Certbot webroot first")
    if WEBROOT.is_symlink() or (WEBROOT.exists() and not WEBROOT.is_dir()):
        parser.error("Refusing unexpected ACME webroot")
    WEBROOT.mkdir(mode=0o755, exist_ok=True)
    install(config)
    print("Staging site enabled. Expected response: 503; no application routes exposed.")


if __name__ == "__main__":
    main()
