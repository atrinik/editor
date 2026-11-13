"""Check archive usability and portable asset policy without game data."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import importlib.util
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT/'tools'/filename)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


assets = module('package_assets', 'package-assets.py')
validation = module('check_prototype_assets', 'check-prototype-assets.py')


class PackageAssetTests(unittest.TestCase):
    def test_portable_policy_paths_do_not_depend_on_host_separator(self):
        for kind, root in [(PureWindowsPath, 'C:/repository'), (PurePosixPath, '/repository')]:
            prefix = kind(root)
            self.assertEqual(validation.portable_relative(prefix/'sample'/'tile.atx1', prefix), 'sample/tile.atx1')

    def test_binary_archive_gets_trusted_entry_point_and_only_authored_data(self):
        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary)
            assets.package_assets(ROOT, stage)
            self.assertEqual((stage/'tools/provision-prototype.py').read_bytes(), (ROOT/'tools/provision-prototype.py').read_bytes())
            self.assertTrue((stage/'samples/prototype-neutral/world.jsonc').is_file())
            self.assertFalse((stage/'samples/prototype-neutral/generate_assets.py').exists())
            self.assertFalse(any(stage.rglob('*.map')))
            self.assertFalse((stage/'recovery').exists())
            self.assertTrue((stage/'asset-provenance.json').is_file())

    def test_every_sample_file_but_the_generator_is_packaged(self):
        # A fixture file left out of the policy ships a sample that does not
        # open; tools/package-linux.sh also opens the staged sample with the
        # packaged binary.
        policy = json.loads((ROOT/'policy/package-assets.json').read_text())
        listed = set(policy['files'])
        sample = ROOT/assets.PREFIX
        present = {path.relative_to(ROOT).as_posix() for path in sample.rglob('*') if path.is_file()}
        unpackaged = sorted(present - listed - {(assets.PREFIX/'generate_assets.py').as_posix()})
        self.assertEqual(unpackaged, [])

    def test_policy_cannot_include_a_parent_or_external_sample(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root/'policy').mkdir()
            stage = root/'stage'
            stage.mkdir()
            (root/'policy/package-assets.json').write_text(json.dumps({'schema_version':1,'sample':'prototype-neutral','files':['../external.map']}))
            with self.assertRaisesRegex(ValueError, 'outside exact allowlist'):
                assets.package_assets(root, stage)
            self.assertEqual(list(stage.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
