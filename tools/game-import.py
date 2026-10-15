#!/usr/bin/env python3
"""Co-located MAME library import worker. Archives are NEVER extracted.

Called only by the bearer-protected control-plane adapter. Paths/executables
come from operator configuration, never from browser input. No downloads.
"""
import contextlib
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
import xml.etree.ElementTree as ET

CHUNK = 4 * 1024 * 1024
MAX_FILE = 16 * 1024**3
QUOTA = 32 * 1024**3


def safe_name(value):
    if not isinstance(value, str) or not re.fullmatch(r'[a-z0-9_][a-z0-9_.-]{0,95}', value) or '..' in value:
        raise ValueError('Use a MAME short name or original lowercase filename, without folders or traversal')
    return value


def identifier(value):
    if str(uuid.UUID(value)) != value:
        raise ValueError('Invalid import identifier')
    return value


def atomic_json(path, value):
    fd, temporary = tempfile.mkstemp(prefix='.4play-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as stream:
            json.dump(value, stream, indent=2)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


@contextlib.contextmanager
def locked(path):
    with open(path, 'a+b') as stream:
        if os.name == 'posix':
            import fcntl
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        else:
            import msvcrt
            stream.seek(0)
            stream.write(b'0')
            stream.flush()
            stream.seek(0)
            msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
        try:
            yield
        finally:
            if os.name == 'posix':
                fcntl.flock(stream, fcntl.LOCK_UN)
            else:
                stream.seek(0)
                msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)


def digest(path):
    result = hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda: stream.read(CHUNK), b''):
            result.update(block)
    return result.hexdigest()


class Importer:
    def __init__(self, config):
        self.config = config
        self.roms = Path(config['rom_root']).resolve(strict=True)
        self.samples = Path(config['sample_root']).resolve(strict=True)
        self.manifest = Path(config['manifest']).resolve(strict=True)
        self.stage = Path(config['stage_root'])
        self.stage.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.stage = self.stage.resolve(strict=True)
        if not self.roms.is_dir() or not self.samples.is_dir() or not self.manifest.is_file():
            raise ValueError('Importer roots/manifest are not configured correctly')
        if self.stage == self.roms or self.stage == self.samples:
            raise ValueError('Staging must be separate from the live library')

    def mame(self, *args, rompath=None, samplepath=None):
        command = [self.config['mame'], '-inipath', self.config['ini']]
        if rompath:
            command += ['-rompath', rompath]
        if samplepath:
            command += ['-samplepath', samplepath]
        return self.run(command + list(args), 120)

    @staticmethod
    def run(command, timeout):
        def limits():
            import resource
            os.nice(10)
            resource.setrlimit(resource.RLIMIT_AS, (4 * 1024**3, 4 * 1024**3))
            resource.setrlimit(resource.RLIMIT_CPU, (timeout, timeout))
            resource.setrlimit(resource.RLIMIT_FSIZE, (16 * 1024 * 1024, 16 * 1024 * 1024))
        # File-backed output avoids accumulating arbitrary subprocess output in RAM.
        with tempfile.TemporaryFile() as output:
            result = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=output,
                                    stderr=subprocess.STDOUT, timeout=timeout, check=False,
                                    preexec_fn=limits if os.name == 'posix' else None)
            output.seek(0)
            text = output.read(8 * 1024 * 1024 + 1)
        if len(text) > 8 * 1024 * 1024:
            raise ValueError('MAME output exceeded the safety limit')
        return result.returncode, text.decode('utf-8', errors='replace')

    def inspect(self, game):
        safe_name(game)
        queue, seen, machines = [game], set(), {}
        while queue:
            name = queue.pop(0)
            if name in seen:
                continue
            seen.add(name)
            if len(seen) > 32:
                raise ValueError('Dependency chain exceeds supported depth')
            code, text = self.mame('-listxml', name)
            if code:
                raise ValueError('MAME does not recognize this game/set name')
            root = ET.fromstring(text)
            for machine in root.findall('machine'):
                machines[machine.get('name')] = machine
            current = machines.get(name)
            if current is None:
                raise ValueError('MAME XML did not identify the requested set')
            for parent in [current.get('romof'), current.get('cloneof')]:
                if parent and parent not in seen:
                    queue.append(safe_name(parent))
        main = machines[game]
        if main.get('isbios') == 'yes' or main.get('isdevice') == 'yes' or main.get('runnable') == 'no':
            raise ValueError('Choose a playable game; BIOS/device archives are supporting files')
        display = main.find('display')
        if display is None or not display.get('width') or not display.get('height'):
            raise ValueError('This game has no supported raster display profile')
        refresh = float(display.get('refresh', '0'))
        if not math.isfinite(refresh) or refresh <= 0:
            raise ValueError('Invalid MAME display refresh rate')
        archives, disks, samples = {}, [], set()
        archives[game] = 'game'
        for name, machine in machines.items():
            if name != game and machine.findall('rom'):
                archives[safe_name(name)] = 'bios' if machine.get('isbios') == 'yes' else 'device' if machine.get('isdevice') == 'yes' else 'parent'
            for disk in machine.findall('disk'):
                if disk.get('status') == 'nodump' or disk.get('optional') == 'yes':
                    continue
                filename = safe_name(disk.get('name', '') + '.chd')
                disks.append({'path': f'{safe_name(name)}/{filename}', 'sha1': disk.get('sha1'), 'kind': 'chd'})
            if machine.findall('sample'):
                samples.add(safe_name(machine.get('sampleof') or name))
        driver = main.find('driver')
        driver_status = driver.get('status', 'unknown') if driver is not None else 'unknown'
        options = []
        for name, kind in archives.items():
            for suffix in ['.zip', '.7z']:
                options.append({'path': name + suffix, 'kind': 'rom', 'role': kind})
        options += disks
        options += [{'path': name + '.zip', 'kind': 'samples', 'role': 'samples'} for name in sorted(samples)]
        for option in options:
            option['installed'] = self.target(option).is_file()
            if option['kind'] == 'rom':
                option['installed'] = any(self.target({**option, 'path': str(Path(option['path']).with_suffix(suffix))}).is_file() for suffix in ('.zip', '.7z'))
        return {'game': game, 'title': main.findtext('description') or game,
                'driver_status': driver_status, 'files': options,
                'note': 'Dependency archives may already be embedded in a non-merged set. The MAME audit, not filename presence, decides readiness.'}

    def target(self, item):
        parts = item['path'].split('/')
        if len(parts) > 2 or any(safe_name(part) != part for part in parts):
            raise ValueError('Invalid library target')
        root = self.samples if item['kind'] == 'samples' else self.roms
        parent = root
        for part in parts[:-1]:
            parent /= part
            if parent.is_symlink() or (parent.exists() and not parent.is_dir()):
                raise ValueError('Library target crosses a symlink or non-directory')
        target = parent / parts[-1]
        if target.is_symlink():
            raise ValueError('Refusing a symlink library target')
        return target

    def folder(self, job):
        path = self.stage / identifier(job)
        if path.is_symlink() or path.resolve().parent != self.stage:
            raise ValueError('Invalid staging path')
        return path

    def load(self, job):
        return json.loads((self.folder(job) / 'job.json').read_text(encoding='utf-8'))

    def save(self, job):
        atomic_json(self.folder(job['id']) / 'job.json', job)

    def status(self, job):
        value = self.load(job)
        for item in value['uploads']:
            path = self.folder(job) / (item['id'] + '.part')
            item['received'] = path.stat().st_size if path.exists() else item['size'] if value['state'] == 'installed' else 0
        return value

    def start(self, game):
        jobs = [p for p in self.stage.iterdir() if p.is_dir() and not p.is_symlink()]
        if sum(1 for p in jobs if (p / 'job.json').exists() and json.loads((p / 'job.json').read_text())['state'] not in ('installed', 'discarded')) >= 16:
            raise ValueError('Too many staged imports; discard unused drafts first')
        plan = self.inspect(game)
        job = {'id': str(uuid.uuid4()), 'state': 'draft', 'plan': plan, 'uploads': [], 'audit': None}
        self.folder(job['id']).mkdir(mode=0o700)
        self.save(job)
        return job

    def add_file(self, job_id, path, kind, size):
        job = self.load(job_id)
        if job['state'] not in ('draft', 'needs_files', 'ready'):
            raise ValueError('Import is busy or already installed')
        matches = [item for item in job['plan']['files'] if item['path'] == path and item['kind'] == kind]
        if not matches or type(size) is not int or not 0 < size <= MAX_FILE:
            raise ValueError('File is not an expected dependency, or exceeds the 16 GiB per-file limit')
        if self.target(matches[0]).exists() or (kind == 'rom' and any(self.target({**matches[0], 'path': str(Path(path).with_suffix(suffix))}).exists() for suffix in ('.zip', '.7z'))):
            raise ValueError('File already exists in the library; it will be reused, never overwritten')
        if kind == 'rom' and any(item['kind'] == 'rom' and Path(item['path']).stem == Path(path).stem and item['path'] != path for item in job['uploads']):
            raise ValueError('Choose one archive format per ROM set, not both ZIP and 7z')
        if len(job['uploads']) >= 64:
            raise ValueError('Too many files in this import')
        if any(item['path'] == path and item['kind'] == kind for item in job['uploads']):
            return self.status(job_id)
        reserved = sum(item['size'] for p in self.stage.iterdir() if p.is_dir() and not p.is_symlink() and (p / 'job.json').exists()
                       for item in json.loads((p / 'job.json').read_text())['uploads']
                       if (p / (item['id'] + '.part')).exists())
        if reserved + size > QUOTA or shutil.disk_usage(self.stage).free < size + 256 * 1024**2:
            raise ValueError('Staging quota/free-space limit reached; discard unused imports')
        file_id = str(uuid.uuid4())
        with open(self.folder(job_id) / (file_id + '.part'), 'xb'):
            pass
        job['uploads'].append({'id': file_id, 'path': path, 'kind': kind, 'size': size})
        job['state'], job['audit'] = 'draft', None
        self.save(job)
        return self.status(job_id)

    def chunk(self, job_id, file_id, offset, data):
        job = self.load(job_id)
        if job['state'] not in ('draft', 'needs_files') or not 0 < len(data) <= CHUNK:
            raise ValueError('Import is not writable or chunk is invalid')
        item = next((item for item in job['uploads'] if item['id'] == identifier(file_id)), None)
        if item is None:
            raise ValueError('Unknown staged file')
        path = self.folder(job_id) / (file_id + '.part')
        if path.is_symlink() or path.stat().st_size != offset or offset + len(data) > item['size']:
            raise ValueError('Upload offset/size mismatch; reload the import to resume')
        if job['state'] != 'draft':
            job['state'], job['audit'] = 'draft', None
            job.pop('error', None)
            self.save(job)
        with open(path, 'ab') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        return {'received': offset + len(data)}

    def begin(self, job_id, action):
        job = self.load(job_id)
        if action == 'audit' and job['state'] not in ('draft', 'ready', 'needs_files', 'auditing'):
            raise ValueError('Import cannot be audited in this state')
        if action == 'install' and (job['state'] not in ('ready', 'installing') or not job.get('audit', {}).get('ok')):
            raise ValueError('Audit must pass before installation')
        job['state'] = 'auditing' if action == 'audit' else 'installing'
        job.pop('error', None)
        self.save(job)
        return job

    def audit(self, job_id):
        job = self.load(job_id)
        folder = self.folder(job_id)
        # Build a private search tree using links to complete staged files only.
        roms, samples = folder / 'roms', folder / 'samples'
        roms.mkdir(exist_ok=True)
        samples.mkdir(exist_ok=True)
        warnings = []
        for item in job['uploads']:
            source = folder / (item['id'] + '.part')
            if source.is_symlink() or source.stat().st_size != item['size']:
                raise ValueError(f'Upload incomplete: {item["path"]}')
            with open(source, 'rb') as stream:
                magic = stream.read(8)
            if (item['path'].endswith('.zip') and not magic.startswith((b'PK\x03\x04', b'PK\x05\x06'))
                    or item['path'].endswith('.7z') and not magic.startswith(b'7z\xbc\xaf\x27\x1c')
                    or item['kind'] == 'chd' and magic != b'MComprHD'):
                raise ValueError(f'File signature does not match its extension: {item["path"]}')
            target = (samples if item['kind'] == 'samples' else roms) / item['path']
            target.parent.mkdir(exist_ok=True)
            if not target.exists():
                os.link(source, target)
            item['sha256'] = digest(source)
            if item['kind'] == 'chd':
                chdman = self.config.get('chdman') or shutil.which('chdman')
                if not chdman:
                    raise ValueError('chdman is required to verify uploaded CHDs; install MAME tools or configure its executable')
                code, output = self.run([chdman, 'verify', '-i', str(source)], 1800)
                if code:
                    raise ValueError('CHD verification failed: ' + output[-8000:])
        # Installed files take precedence: a staged upload cannot mask a broken
        # installed archive that this no-overwrite workflow would not replace.
        code, output = self.mame('-verifyroms', job['plan']['game'], rompath=f'{self.roms};{roms}')
        samples_ok, sample_output = True, ''
        if any(item['kind'] == 'samples' for item in job['plan']['files']):
            sample_code, sample_output = self.mame('-verifysamples', job['plan']['game'], samplepath=f'{self.samples};{samples}')
            samples_ok = sample_code == 0
            if not samples_ok:
                warnings.append('Optional audio samples are missing or invalid; the game may have missing sounds.')
        if job['plan']['driver_status'] == 'preliminary':
            warnings.append('This MAME build marks the game not working; installation into the visible catalog is blocked.')
        elif job['plan']['driver_status'] == 'imperfect':
            warnings.append('MAME marks this game imperfect; a successful ROM audit does not guarantee perfect gameplay/audio.')
        supplied_samples = any(item['kind'] == 'samples' for item in job['uploads'])
        job['audit'] = {'ok': code == 0 and (samples_ok or not supplied_samples) and job['plan']['driver_status'] != 'preliminary', 'roms_ok': code == 0,
                        'samples_ok': samples_ok, 'warnings': warnings, 'output': output[-16000:], 'sample_output': sample_output[-8000:]}
        job['state'] = ('installing' if job['state'] == 'installing' else 'ready') if job['audit']['ok'] else 'needs_files'
        job.pop('error', None)
        self.save(job)
        return job

    def install(self, job_id):
        # Re-audit under the same job lock before touching the live library.
        job = self.audit(job_id)
        if not job['audit']['ok']:
            return job
        self.begin(job_id, 'install')
        for item in job['uploads']:
            source, target = self.folder(job_id) / (item['id'] + '.part'), self.target(item)
            target.parent.mkdir(exist_ok=True)
            if target.exists():
                if digest(target) != item['sha256']:
                    raise ValueError(f'Existing file differs; refusing overwrite: {item["path"]}')
                continue
            # Destination-local temp + exclusive hard-link publication handles
            # different staging filesystems and never replaces a live archive.
            fd, temporary = tempfile.mkstemp(prefix='.4play-import-', dir=target.parent)
            try:
                with os.fdopen(fd, 'wb') as stream, open(source, 'rb') as uploaded:
                    shutil.copyfileobj(uploaded, stream, CHUNK)
                    stream.flush()
                    os.fsync(stream.fileno())
                os.link(temporary, target)
            finally:
                os.unlink(temporary)
        with locked(self.manifest.with_name(self.manifest.name + '.import.lock')):
            manifest = json.loads(self.manifest.read_text(encoding='utf-8'))
            if manifest.get('schema_version') != 1 or not isinstance(manifest.get('games'), list):
                raise ValueError('Unsupported catalog manifest')
            game = job['plan']['game']
            matching = next((entry for entry in manifest['games'] if entry['id'] == game), None)
            if matching and matching.get('rom_name') != game:
                raise ValueError('Catalog ID already belongs to another ROM')
            backup = self.manifest.with_name(self.manifest.name + f'.before-{job_id}.bak')
            if not backup.exists():
                with open(backup, 'x', encoding='utf-8') as stream:
                    json.dump(manifest, stream, indent=2)
            if not matching:
                manifest['games'].append({'id': game, 'rom_name': game})
                atomic_json(self.manifest, manifest)
        job['state'] = 'installed'
        job['message'] = 'Files installed and manifest updated. The runtime host will verify and publish the updated catalog on its next heartbeat.'
        self.save(job)
        # Only our staged file names are removed; live files are never deleted.
        self.discard_files(job)
        return job

    def discard_files(self, job):
        for item in job['uploads']:
            source = self.folder(job['id']) / (identifier(item['id']) + '.part')
            source.unlink(missing_ok=True)
            link = self.folder(job['id']) / ('samples' if item['kind'] == 'samples' else 'roms') / item['path']
            link.unlink(missing_ok=True)

    def discard(self, job_id):
        job = self.load(job_id)
        if job['state'] == 'installed':
            raise ValueError('Installed imports cannot be uninstalled from staging')
        self.discard_files(job)
        job['state'] = 'discarded'
        self.save(job)
        return job


def main():
    config, action = json.loads(sys.argv[1]), sys.argv[2]
    request = json.loads(sys.stdin.buffer.readline(CHUNK))
    importer = Importer(config)
    job_id = request.get('job')
    if action == 'start':
        with locked(importer.stage / '.intake.lock'):
            result = importer.start(request['game'])
    elif action == 'status':
        result = importer.status(job_id)
    else:
        with locked(importer.folder(job_id) / '.lock'):
            try:
                result = execute_job_action(importer, action, job_id, request)
            except Exception as error:
                if action in ('audit', 'install'):
                    job = importer.load(job_id)
                    if job['state'] in ('auditing', 'installing'):
                        job['state'], job['error'] = 'needs_files', str(error)
                        importer.save(job)
                raise
    print(json.dumps(result))


def execute_job_action(importer, action, job_id, request):
    if action == 'add':
        with locked(importer.stage / '.intake.lock'):
            return importer.add_file(job_id, request['path'], request['kind'], request['size'])
    if action == 'chunk':
        return importer.chunk(job_id, request['file'], request['offset'], sys.stdin.buffer.read(CHUNK + 1))
    if action == 'begin':
        return importer.begin(job_id, request['action'])
    if action in ('audit', 'install'):
        expected = 'auditing' if action == 'audit' else 'installing'
        if importer.load(job_id)['state'] != expected:
            raise ValueError('Import operation already completed or was superseded')
        return getattr(importer, action)(job_id)
    if action == 'discard':
        return importer.discard(job_id)
    raise ValueError('Unknown import action')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print(json.dumps({'error': str(error)}))
        sys.exit(1)
