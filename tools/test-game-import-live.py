#!/usr/bin/env python3
"""Linux integration: real upload/audit/install/catalog refresh in a private sandbox.

Requires built release binaries and your own matching game/BIOS archives.
Never writes to the configured live ROM directory or launches a MAME game.
"""
import argparse
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import time
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mame', required=True)
    parser.add_argument('--ini', required=True, help='Directory containing the reference mame.ini (read only)')
    parser.add_argument('--rom', required=True)
    parser.add_argument('--bios', required=True)
    parser.add_argument('--game', default='aof')
    args = parser.parse_args()
    if os.name != 'posix':
        raise SystemExit('Run this integration test on the Linux runtime host')
    repo = Path(__file__).resolve().parents[1]
    secret = 'isolated-import-test-only-token'
    with tempfile.TemporaryDirectory(prefix='4play-import-http-') as temporary:
        root = Path(temporary)
        for name in ['roms', 'samples', 'ini']:
            (root / name).mkdir()
        text = (Path(args.ini) / 'mame.ini').read_text()
        text = re.sub(r'^rompath\s+.*$', f'rompath {root / "roms"}', text, flags=re.MULTILINE)
        text = re.sub(r'^samplepath\s+.*$', f'samplepath {root / "samples"}', text, flags=re.MULTILINE)
        (root / 'ini/mame.ini').write_text(text)
        manifest = root / 'catalog.json'
        manifest.write_text('{"schema_version":1,"games":[]}')
        with socket.socket() as address:
            address.bind(('127.0.0.1', 0))
            port = address.getsockname()[1]
        base = f'http://127.0.0.1:{port}'
        env = {key:value for key,value in os.environ.items() if not key.startswith('FOURPLAY_')}
        env.update({
            'FOURPLAY_CONTROL_PLANE_BIND': f'127.0.0.1:{port}', 'FOURPLAY_CONTROL_PLANE_URL':base,
            'FOURPLAY_CONTROL_PLANE_DATABASE':str(root/'control.sqlite3'),
            'FOURPLAY_SEAT_API_TOKEN':secret,'FOURPLAY_RUNTIME_HOST_API_TOKEN':secret,
            'FOURPLAY_RUNTIME_HOST_ID':'import-test','FOURPLAY_RUNTIME_HOST_ADDRESS':'127.0.0.1',
            'FOURPLAY_HEARTBEAT_SECONDS':'1',
            'FOURPLAY_MAME_PATH':args.mame,'FOURPLAY_MAME_INI_PATH':str(root/'ini'),
            'FOURPLAY_CATALOG_PATH':str(manifest), 'FOURPLAY_GAME_IMPORT_ENABLED':'1',
            'FOURPLAY_GAME_IMPORT_WORKER':str(repo/'tools/game-import.py'),
            'FOURPLAY_GAME_IMPORT_ROM_ROOT':str(root/'roms'),'FOURPLAY_GAME_IMPORT_SAMPLE_ROOT':str(root/'samples'),
            'FOURPLAY_GAME_IMPORT_STAGE_ROOT':str(root/'stage'),
        })
        def request(path, value=None, method=None):
            data = value if isinstance(value, bytes) else json.dumps(value).encode() if value is not None else None
            req = urllib.request.Request(base+path,data=data,method=method or ('POST' if data is not None else 'GET'),
                headers={'Authorization':f'Bearer {secret}','Content-Type':'application/octet-stream' if isinstance(value,bytes) else 'application/json'})
            with urllib.request.urlopen(req,timeout=150) as response:
                return json.load(response)
        def wait(predicate, timeout=40):
            deadline=time.monotonic()+timeout
            while time.monotonic()<deadline:
                try:
                    value=predicate()
                    if value:return value
                except (OSError, ValueError):pass
                time.sleep(.2)
            raise RuntimeError('Timed out waiting for isolated integration state')
        processes=[]
        with open(root/'services.log','wb') as log:
            try:
                processes.append(subprocess.Popen([str(repo/'target/release/control-plane-server')],env=env,cwd=root,stdout=log,stderr=log))
                wait(lambda:request('/health'))
                processes.append(subprocess.Popen([str(repo/'target/release/runtime-host-agent')],env=env,cwd=root,stdout=log,stderr=log))
                wait(lambda:request('/api/v1/runtime-hosts')['hosts'])
                job=request('/api/v1/admin/game-imports',{'game':args.game})
                for archive in [Path(args.rom),Path(args.bios)]:
                    job=request(f'/api/v1/admin/game-imports/{job["id"]}/files',{'path':archive.name,'kind':'rom','size':archive.stat().st_size})
                    item=next(item for item in job['uploads'] if item['path']==archive.name)
                    offset=0
                    with open(archive,'rb') as source:
                        while chunk:=source.read(4*1024*1024):
                            result=request(f'/api/v1/admin/game-imports/{job["id"]}/files/{item["id"]}/{offset}',chunk,'PUT')
                            offset=result['received']
                request(f'/api/v1/admin/game-imports/{job["id"]}/audit',{})
                def finished():
                    value=request(f'/api/v1/admin/game-imports/{job["id"]}')
                    return value if value['state'] not in ('auditing','installing') else None
                audited=wait(finished,150)
                assert audited['state']=='ready', audited
                request(f'/api/v1/admin/game-imports/{job["id"]}/install',{})
                installed=wait(finished,150)
                assert installed['state']=='installed',installed
                visible=wait(lambda:next((game for game in request('/api/v1/games')['games'] if game['id']==args.game),None))
                assert visible['availability'][0]['runtime_host_id']=='import-test'
                assert (root/'roms'/Path(args.rom).name).is_file()
                print('PASS: authenticated chunk upload, real MAME/BIOS audit, explicit sandbox installation, automatic host-agent catalog refresh; no live games/ROMs modified.')
            except Exception:
                log.flush()
                print((root/'services.log').read_text()[-8000:])
                raise
            finally:
                for process in reversed(processes):
                    process.terminate()
                    try:process.wait(timeout=5)
                    except subprocess.TimeoutExpired:process.kill();process.wait()


if __name__=='__main__':main()
