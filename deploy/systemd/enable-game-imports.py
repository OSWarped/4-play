#!/usr/bin/env python3
"""One-time operator setup. Does not restart services or change ROMs/MAME config."""
import argparse
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import subprocess
import tempfile
import uuid


def environment(text):
    result = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        key, separator, value = line.partition('=')
        if not separator or not re.fullmatch(r'[A-Z0-9_]+', key):
            raise ValueError('Expected simple KEY=value entries in EnvironmentFile')
        value = value.strip()
        if value.startswith(('"', "'")):
            if value[-1:] != value[:1]:
                raise ValueError('Unsupported multiline/quoted environment setting')
            value = value[1:-1]
        result[key] = value
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--env-file', default='/etc/4play/4play.env')
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    if os.geteuid() != 0:
        raise ValueError('Run this setup with sudo; the private service environment is root-owned')
    env_path = Path(args.env_file).resolve(strict=True)
    text = env_path.read_text()
    config = environment(text)
    users = [subprocess.check_output(['systemctl', 'show', service, '--property=User', '--value'], text=True).strip()
             for service in ['4play-control-plane.service', '4play-runtime-host-agent.service']]
    if not users[0] or users[0] != users[1] or users[0] == 'root':
        raise ValueError('Co-located control plane and runtime host must use the same non-root service user')
    account = pwd.getpwnam(users[0])
    for key in ['FOURPLAY_MAME_PATH', 'FOURPLAY_MAME_INI_PATH', 'FOURPLAY_CATALOG_PATH', 'FOURPLAY_RUNTIME_HOST_ID']:
        if not config.get(key):
            raise ValueError(f'{key} must already be configured')
    ini = (Path(config['FOURPLAY_MAME_INI_PATH']) / 'mame.ini').read_text()
    paths = {}
    for line in ini.splitlines():
        fields = line.split(None, 1)
        if len(fields) == 2 and fields[0] in ('rompath', 'samplepath'):
            paths[fields[0]] = fields[1].strip().strip('"').replace('$HOME', account.pw_dir).replace('~', account.pw_dir).split(';')
    if len(paths.get('rompath', [])) != 1 or not paths.get('samplepath'):
        raise ValueError('This first importer supports one co-located ROM root and the first configured sample path')
    if not Path(paths['rompath'][0]).is_absolute() or not Path(paths['samplepath'][0]).is_absolute():
        raise ValueError('Use absolute ROM/sample paths in mame.ini')
    roms = Path(paths['rompath'][0]).resolve(strict=True)
    samples = Path(paths['samplepath'][0]).resolve()
    if not roms.is_dir() or not samples.is_absolute():
        raise ValueError('ROM/sample paths must be absolute local directories')
    original = Path(config['FOURPLAY_CATALOG_PATH']).resolve(strict=True)
    manifest = json.loads(original.read_text())
    if manifest.get('schema_version') != 1 or not isinstance(manifest.get('games'), list):
        raise ValueError('Expected an existing version-1 catalog manifest')
    private = Path(account.pw_dir) / '.local/share/4play'
    catalog = private / 'catalog.json'
    worker = Path(__file__).resolve().parents[2] / 'tools/game-import.py'
    if not worker.is_file() or not shutil.which('chdman'):
        raise ValueError('Worker or chdman missing; pull the code and install MAME tools first')
    if catalog.exists() and catalog.resolve() != original and catalog.read_bytes() != original.read_bytes():
        raise ValueError('Private catalog already differs from the configured manifest; refusing to replace it')
    if catalog.exists() and catalog.stat().st_uid != account.pw_uid:
        raise ValueError('The existing private catalog must be owned by the service user')
    updates = {
        'FOURPLAY_GAME_IMPORT_ENABLED': '1', 'FOURPLAY_GAME_IMPORT_WORKER': str(worker),
        'FOURPLAY_GAME_IMPORT_ROM_ROOT': str(roms), 'FOURPLAY_GAME_IMPORT_SAMPLE_ROOT': str(samples),
        'FOURPLAY_GAME_IMPORT_STAGE_ROOT': str(private / 'imports'), 'FOURPLAY_GAME_IMPORT_CHDMAN': shutil.which('chdman'),
        'FOURPLAY_CATALOG_PATH': str(catalog),
    }
    if args.dry_run:
        print(json.dumps(updates, indent=2))
        return
    for directory in [private, private / 'imports', samples]:
        if directory.exists():
            if not directory.is_dir() or directory.stat().st_uid != account.pw_uid:
                raise ValueError(f'{directory} must be a directory owned by the service user; setup will not take ownership of existing directories')
            continue
        missing, parent = [], directory
        while not parent.exists():
            missing.append(parent)
            parent = parent.parent
        for path in reversed(missing):
            path.mkdir()
            os.chown(path, account.pw_uid, account.pw_gid)
    os.chmod(private / 'imports', 0o700)
    if not catalog.exists():
        with open(catalog, 'xb') as stream:
            stream.write(original.read_bytes())
        os.chown(catalog, account.pw_uid, account.pw_gid)
        os.chmod(catalog, 0o640)
    backup = env_path.with_name(env_path.name + '.before-game-import-' + str(uuid.uuid4()))
    shutil.copy2(env_path, backup)
    os.chmod(backup, 0o600)
    lines = [line for line in text.splitlines() if line.partition('=')[0].strip() not in updates]
    lines += [f'{key}={json.dumps(value)}' for key, value in updates.items()]
    fd, temporary = tempfile.mkstemp(prefix='.4play-env-', dir=env_path.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            stream.write('\n'.join(lines) + '\n')
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, 0o600)
        os.replace(temporary, env_path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    print(f'Importer configured. Private environment backup: {backup}')
    print('Original catalog and ROMs were preserved. No services were restarted.')
    print('When no games are active, restart control plane and runtime host agent, then reload Admin.')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        raise SystemExit(str(error))
