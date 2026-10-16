#!/usr/bin/env python3
"""Real MAME NO_DUMP exceptions and negative tests; private state/ROM copies only."""
import argparse
from pathlib import Path
import runpy
import subprocess
import tempfile
import xml.etree.ElementTree as ET
import zipfile

probe = runpy.run_path(str(Path(__file__).with_name('test-mame-headless-warnings.py')))['probe']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mame', required=True)
    parser.add_argument('--ini', required=True)
    parser.add_argument('--rom-root', required=True)
    args = parser.parse_args()
    for game, expected, size in [('rthunder', 'hd637a01x0p.e3', 4096), ('rampage', 'e36a31axnaxqd.u15.bin', 1)]:
        xml = subprocess.check_output([args.mame, '-listxml', game], timeout=120)
        machine = next(m for m in ET.fromstring(xml).findall('machine') if m.get('name') == game)
        undumped = [rom for rom in machine.findall('rom') if rom.get('status') == 'nodump']
        assert [(rom.get('name'), int(rom.get('size'))) for rom in undumped] == [(expected, size)]
        audit = subprocess.run([args.mame, '-inipath', args.ini, '-verifyroms', game], capture_output=True, text=True, timeout=120)
        assert audit.returncode == 0, audit.stdout + audit.stderr
        for approval, acknowledge in [('', '1'), ('contra', '1'), (game, '0')]:
            counts, log = probe(args.mame, args.ini, game, acknowledge, 3, approval=approval)
            assert counts == [0, 0] and '4-Play acknowledged' not in log, log
        counts, log = probe(args.mame, args.ini, game, '1', 8, approval=game)
        assert all(count > 0 for count in counts), log
        assert '4-Play acknowledged non-critical startup warning:' in log, log
        print(f'{game}: PASS exact opt-in streams real audio/video {counts}; missing/wrong/disabled approval stays blocked.', flush=True)
        # Build malformed private archives without changing/extracting installed ROMs.
        required = next(rom.get('name') for rom in machine.findall('rom') if rom.get('sha1') and rom.get('optional') != 'yes')
        for mode in ['checksum', 'missing']:
            with tempfile.TemporaryDirectory(prefix='4play-undumped-negative-') as folder:
                with zipfile.ZipFile(Path(args.rom_root) / (game + '.zip')) as source, zipfile.ZipFile(Path(folder) / (game + '.zip'), 'w') as target:
                    assert required in source.namelist(), 'Probe expects standalone parent ZIPs'
                    for info in source.infolist():
                        if info.filename == required and mode == 'missing':
                            continue
                        assert info.file_size <= 64 * 1024 * 1024, 'Probe file-size safety limit'
                        data = source.read(info)
                        if info.filename == required:
                            data = bytes([data[0] ^ 1]) + data[1:]
                        target.writestr(info, data)
                counts, log = probe(args.mame, args.ini, game, '1', 3, approval=game, rompath=folder)
                assert counts == [0, 0] and '4-Play acknowledged' not in log, log
                assert ('WRONG CHECKSUMS' in log if mode == 'checksum' else 'NOT FOUND' in log), log
                print(f'{game}: PASS {mode} failure remains blocked; private ROM copies removed.', flush=True)
    counts, log = probe(args.mame, args.ini, 'contra', '1', 3, approval='contra')
    assert counts == [0, 0] and '4-Play acknowledged' not in log, log
    print('PASS: an unapproved game cannot opt itself into the undumped exception.', flush=True)


if __name__ == '__main__':
    main()
