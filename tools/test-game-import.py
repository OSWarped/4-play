import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import uuid

spec = importlib.util.spec_from_file_location('game_import', Path(__file__).with_name('game-import.py'))
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)

XML = '''<mame><machine name="fighter" romof="parent" sampleof="fighter">
<description>Fighter (USA)</description><display width="320" height="224" refresh="60"/>
<driver status="good"/><sample name="hit"/><disk name="fightdisk" sha1="abc"/>
</machine><machine name="qsound_hle" isdevice="yes"><rom name="dsp.bin"/></machine></mame>'''
PARENT = '''<mame><machine name="parent" romof="neogeo"><rom name="p.bin"/></machine>
<machine name="neogeo" isbios="yes"><rom name="bios.bin"/></machine></mame>'''


class FakeImporter(worker.Importer):
    rom_ok = True
    samples_ok = True
    chd_ok = True
    def mame(self, *args, **kwargs):
        if args[0] == '-listxml':
            return 0, XML if args[1] == 'fighter' else PARENT
        if args[0] == '-verifyroms':
            return (0, 'romset fighter is good') if self.rom_ok else (2, 'qsound_hle/dsp.bin NOT FOUND')
        return (0, 'samples good') if self.samples_ok else (2, 'samples missing')
    def run(self, command, timeout):
        return (0, 'CHD verified') if self.chd_ok else (1, 'bad CHD hash')


class Imports(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        for name in ['roms', 'samples']:
            (self.root / name).mkdir()
        self.manifest = self.root / 'catalog.json'
        self.manifest.write_text(json.dumps({'schema_version': 1, 'games': [{'id': 'existing', 'rom_name': 'existing'}]}))
        self.engine = FakeImporter(dict(rom_root=str(self.root/'roms'), sample_root=str(self.root/'samples'),
            stage_root=str(self.root/'staged'), manifest=str(self.manifest), mame='fake', ini='fake', chdman='fake'))
    def tearDown(self):
        self.temp.cleanup()
    def upload(self, job, path='fighter.zip', kind='rom', data=b'PK\x03\x04test'):
        job = self.engine.add_file(job['id'], path, kind, len(data))
        item = next(item for item in job['uploads'] if item['path'] == path)
        self.engine.chunk(job['id'], item['id'], 0, data)
        return self.engine.load(job['id'])

    def test_metadata_discovers_parent_bios_device_chd_and_samples(self):
        plan = self.engine.inspect('fighter')
        self.assertEqual(plan['title'], 'Fighter (USA)')
        self.assertTrue(any(item['path']=='neogeo.zip' and item['role']=='bios' for item in plan['files']))
        self.assertTrue(any(item['path']=='qsound_hle.zip' and item['role']=='device' for item in plan['files']))
        self.assertTrue(any(item['path']=='parent.zip' for item in plan['files']))
        self.assertTrue(any(item['path']=='fighter/fightdisk.chd' for item in plan['files']))
        self.assertTrue(any(item['path']=='fighter.zip' and item['kind']=='samples' for item in plan['files']))

    def test_upload_offsets_resume_bounds_and_signatures(self):
        job = self.engine.start('fighter')
        job = self.engine.add_file(job['id'], 'fighter.zip', 'rom', 8)
        item = job['uploads'][0]
        self.engine.chunk(job['id'],item['id'],0,b'PK\x03\x04')
        self.assertEqual(self.engine.status(job['id'])['uploads'][0]['received'],4)
        with self.assertRaises(ValueError): self.engine.chunk(job['id'],item['id'],0,b'no')
        with self.assertRaises(ValueError): self.engine.audit(job['id'])
        self.engine.chunk(job['id'],item['id'],4,b'test')
        self.assertTrue(self.engine.audit(job['id'])['audit']['ok'])
        other = self.upload(self.engine.start('fighter'),data=b'notazip!')
        with self.assertRaises(ValueError): self.engine.audit(other['id'])

    def test_missing_dependencies_block_install_and_retry_preserves_draft(self):
        job = self.upload(self.engine.start('fighter'))
        self.engine.rom_ok = False
        result = self.engine.audit(job['id'])
        self.assertEqual(result['state'],'needs_files')
        self.assertIn('NOT FOUND',result['audit']['output'])
        with self.assertRaises(ValueError): self.engine.begin(job['id'],'install')
        job = self.upload(result,'qsound_hle.zip')
        self.assertEqual(self.engine.status(job['id'])['uploads'][0]['received'],8)
        self.engine.rom_ok = True
        self.assertEqual(self.engine.audit(job['id'])['state'],'ready')

    def test_install_preserves_catalog_metadata_and_never_overwrites(self):
        job = self.upload(self.engine.start('fighter'))
        self.engine.audit(job['id']); self.engine.begin(job['id'],'install')
        result = self.engine.install(job['id'])
        self.assertEqual(result['state'],'installed')
        self.assertEqual((self.root/'roms/fighter.zip').read_bytes(),b'PK\x03\x04test')
        self.assertEqual(len(json.loads(self.manifest.read_text())['games']),2)
        self.assertTrue(list(self.root.glob('catalog.json.before-*.bak')))
        self.assertFalse(list(self.engine.folder(job['id']).glob('*.part')))
        new = self.engine.start('fighter')
        with self.assertRaises(ValueError): self.engine.add_file(new['id'],'fighter.zip','rom',8)

    def test_changed_live_file_is_not_masked_or_overwritten(self):
        job = self.upload(self.engine.start('fighter'))
        self.engine.audit(job['id']); self.engine.begin(job['id'],'install')
        (self.root/'roms/fighter.zip').write_bytes(b'keep installed data')
        with self.assertRaises(ValueError): self.engine.install(job['id'])
        self.assertEqual((self.root/'roms/fighter.zip').read_bytes(),b'keep installed data')

    def test_chd_integrity_and_optional_samples(self):
        job = self.upload(self.engine.start('fighter'),'fighter/fightdisk.chd','chd',b'MComprHDdata')
        self.engine.chd_ok = False
        with self.assertRaises(ValueError): self.engine.audit(job['id'])
        self.engine.chd_ok = True; self.engine.samples_ok = False
        result = self.engine.audit(job['id'])
        self.assertTrue(result['audit']['ok']); self.assertFalse(result['audit']['samples_ok'])
        job = self.upload(result,'fighter.zip','samples')
        self.assertFalse(self.engine.audit(job['id'])['audit']['ok'])

    def test_traversal_unexpected_targets_and_discard_are_scoped(self):
        job = self.engine.start('fighter')
        for path in ['../fighter.zip','/fighter.zip','unrelated.zip','fighter/../../evil.chd']:
            with self.assertRaises(ValueError): self.engine.add_file(job['id'],path,'rom',8)
        with self.assertRaises(ValueError): self.engine.folder('../roms')
        for name in ['-listxml','*','../fighter','a/b']:
            with self.assertRaises(ValueError): self.engine.inspect(name)
        job = self.upload(job)
        (self.root/'roms/keep.zip').write_bytes(b'keep')
        self.engine.discard(job['id'])
        self.assertFalse(list(self.engine.folder(job['id']).glob('*.part')))
        self.assertEqual((self.root/'roms/keep.zip').read_bytes(),b'keep')


if __name__ == '__main__':
    unittest.main()
