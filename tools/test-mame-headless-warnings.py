#!/usr/bin/env python3
"""Linux diagnostic: private state/FIFOs, no live session or persistence changes.

Do NOT use -seconds_to_run/-str: short runs bypass startup screens themselves
and would make this regression test pass without the fix.
"""
import argparse
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time


def probe(mame, ini, game, acknowledge, seconds, raw_audio=True, approval="", rompath=None):
    with tempfile.TemporaryDirectory(prefix='4play-warning-probe-') as directory:
        root = Path(directory)
        paths = [root / 'video.raw', root / 'audio.pcm']
        readers = []
        for path in paths:
            os.mkfifo(path)
            readers.append(os.open(path, os.O_RDONLY | os.O_NONBLOCK))
        env = os.environ.copy()
        env.update(SDL_VIDEODRIVER='offscreen', FOURPLAY_MAME_ACK_WARNINGS=acknowledge)
        env['FOURPLAY_MAME_ACK_UNDUMPED'] = approval
        command = [mame, '-inipath', ini, '-skip_gameinfo', '-sound', 'none', '-noautosave',
                   '-rawvideowrite', str(paths[0])]
        if raw_audio:
            command.extend(['-rawaudiowrite', str(paths[1])])
        if rompath:
            command.extend(['-rompath', str(rompath)])
        for option in ['cfg_directory', 'nvram_directory', 'state_directory', 'snapshot_directory', 'diff_directory']:
            target = root / option
            target.mkdir()
            command.extend(['-' + option, str(target)])
        counts = [0, 0]
        with tempfile.TemporaryFile() as output:
            process = subprocess.Popen(command + [game], cwd=root, env=env,
                                       stdin=subprocess.DEVNULL, stdout=output, stderr=output,
                                       start_new_session=True)
            try:
                deadline = time.monotonic() + seconds
                while time.monotonic() < deadline and process.poll() is None:
                    for index, reader in enumerate(readers):
                        try:
                            counts[index] += len(os.read(reader, 1024 * 1024))
                        except BlockingIOError:
                            pass
                    time.sleep(.002)
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait()
                for reader in readers:
                    os.close(reader)
            output.seek(0)
            text = output.read().decode(errors='replace')
        return counts, text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mame', required=True)
    parser.add_argument('--ini', required=True)
    parser.add_argument('--games', nargs='+', default=['elvact2u', 'indytemp', 'kidnikiu'])
    parser.add_argument('--knownbad-game', help='Optional installed set with NO_DUMP/BAD_DUMP warnings (e.g. contra)')
    args = parser.parse_args()
    if os.name != 'posix':
        raise SystemExit('Run on the Linux MAME runtime host')
    for game in args.games:
        off, off_log = probe(args.mame, args.ini, game, '0', 4)
        on, on_log = probe(args.mame, args.ini, game, '1', 8)
        print(f'{game}: opt-out bytes={off}, opt-in bytes={on}', flush=True)
        assert off == [0, 0], off_log
        assert all(count > 0 for count in on), on_log
        assert '4-Play acknowledged non-critical startup warning:' in on_log, on_log
        assert '4-Play acknowledged' not in off_log, off_log
        print('PASS: only explicit opt-in unblocks real audio/video; isolated state removed.', flush=True)
    for value, audio in [('true', True), ('1', False)]:
        counts, log = probe(args.mame, args.ini, args.games[0], value, 4, raw_audio=audio)
        assert counts == [0, 0], log
        assert '4-Play acknowledged' not in log, log
    print('PASS: invalid opt-in and missing raw-audio path do not bypass warnings.', flush=True)
    if args.knownbad_game:
        counts, log = probe(args.mame, args.ini, args.knownbad_game, '1', 4)
        assert counts == [0, 0], log
        assert '4-Play acknowledged' not in log, log
        print('PASS: supplied known-bad/undumped ROM set remains blocked.', flush=True)


if __name__ == '__main__':
    main()
