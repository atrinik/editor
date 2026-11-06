"""Check private fresh-root provisioning and refusal without source mutation."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import importlib.util
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('provision-prototype.py')
spec = importlib.util.spec_from_file_location('prototype_provision', SCRIPT)
provision = importlib.util.module_from_spec(spec)
spec.loader.exec_module(provision)


@unittest.skipUnless(sys.platform == 'linux', 'Linux descriptor provisioning')
class ProvisionTests(unittest.TestCase):
    def test_private_fresh_copy_preserves_original_and_refuses_existing(self):
        with tempfile.TemporaryDirectory(dir=os.environ.get('TILEWRIGHT_PERSISTENT_TEST_ROOT')) as temporary:
            root = Path(temporary)
            fd = provision.directory(root)
            try:
                try:
                    provision.persistent_mount(fd)
                except ValueError as error:
                    self.skipTest(f'persistent test root unavailable: {error}')
            finally:
                os.close(fd)
            source = root / 'source'
            source.mkdir()
            (source / 'world.jsonc').write_bytes(b'{"original": true}\n')
            (source / 'nested').mkdir()
            (source / 'nested' / 'data').write_bytes(b'data')
            destination = root / 'session'
            command = [sys.executable, str(SCRIPT), '--world', str(source), '--destination', str(destination)]
            subprocess.run(command, check=True, capture_output=True)
            self.assertEqual((source / 'world.jsonc').read_bytes(), b'{"original": true}\n')
            self.assertEqual((destination / 'world/world.jsonc').read_bytes(), b'{"original": true}\n')
            for path in (destination, destination/'world', destination/'recovery', destination/'evidence', destination/'world/nested'):
                self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE((destination/'world/world.jsonc').stat().st_mode), 0o600)
            self.assertNotEqual((destination/'world').stat().st_ino, (destination/'recovery').stat().st_ino)
            before = (destination/'provision.json').read_bytes()
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual((destination/'provision.json').read_bytes(), before)

    def test_volatile_tmpfs_is_refused_before_creating_a_session(self):
        with tempfile.TemporaryDirectory(dir='/dev/shm' if Path('/dev/shm').is_dir() and os.access('/dev/shm', os.W_OK) else '/tmp') as temporary:
            root = Path(temporary)
            fd = provision.directory(root)
            try:
                try:
                    provision.persistent_mount(fd)
                except ValueError:
                    pass
                else:
                    self.skipTest('no writable volatile test mount available')
            finally:
                os.close(fd)
            source = root/'source'
            source.mkdir()
            destination = root/'session'
            result = subprocess.run([sys.executable,str(SCRIPT),'--world',str(source),'--destination',str(destination)],capture_output=True)
            self.assertNotEqual(result.returncode,0)
            self.assertFalse(destination.exists())

    def test_editor_launch_requires_explicit_font_pack_before_creating_state(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'
            source.mkdir()
            result = subprocess.run([sys.executable, str(SCRIPT), '--world', str(source),
                '--destination', str(root / 'session'), '--editor', str(root / 'editor')], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'explicit --font-pack', result.stderr)
            self.assertFalse((root / 'session').exists())

    def test_copy_refuses_links_and_retains_any_partial_data(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root/'source'
            source.mkdir()
            outside = root/'outside'
            outside.write_bytes(b'outside')
            (source/'a-link').symlink_to(outside)
            destination = root/'destination'
            destination.mkdir(mode=0o700)
            fd = provision.directory(destination)
            try:
                with self.assertRaisesRegex(ValueError, 'link or special'):
                    provision.copy_tree(source, fd)
            finally:
                os.close(fd)
            self.assertEqual(outside.read_bytes(), b'outside')
            self.assertEqual(list(destination.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
