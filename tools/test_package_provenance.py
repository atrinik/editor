"""Regression proof for dependency provenance after registry updates."""
# Copyright (c) 2026 Atrinik contributors. SPDX-License-Identifier: MIT
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('package_provenance', Path(__file__).with_name('package-provenance.py'))
provenance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(provenance)
NAME = 'atrinik-scene'
REVISION = 'a' * 40
CHECKSUM = 'b' * 64
LEGACY = {'git': 'https://github.com/atrinik/renderer', 'rev': REVISION, 'version': '0.1.0'}


class PackageProvenanceTests(unittest.TestCase):
    def fixture(self, root, registry=True, version='0.2.0', checksum=CHECKSUM, revision=REVISION):
        (root / 'policy').mkdir()
        (root / 'policy/released-dependencies.json').write_text(json.dumps({'schema_version': 1, 'dependencies': [
            {'name': NAME, 'repository': 'atrinik/renderer', 'legacy_git': LEGACY}]}))
        if registry:
            declaration = '{ version = "=' + version + '", default-features = false, features = ["preview"] }'
            source = provenance.REGISTRY
            extra = f'checksum = "{checksum}"\n' if checksum else ''
        else:
            declaration = '{ git = "' + LEGACY['git'] + '", rev = "' + REVISION + '", version = "0.1.0" }'
            source = f'git+{LEGACY["git"]}?rev={revision}#{revision}'
            extra = ''
        (root / 'Cargo.toml').write_text(f'[workspace.dependencies]\n{NAME} = {declaration}\n')
        (root / 'Cargo.lock').write_text(f'[[package]]\nname = "{NAME}"\nversion = "{version}"\nsource = "{source}"\n{extra}')

    def test_updated_registry_version_and_checksum_replace_old_git_revision(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            result = provenance.dependency_provenance(root)
            dependency = result['dependencies'][0]
            self.assertEqual(dependency, {'name': NAME, 'repository': 'atrinik/renderer',
                'version': '0.2.0', 'source': provenance.REGISTRY, 'checksum': CHECKSUM})
            self.assertEqual(result['cargo_lock_sha256'], hashlib.sha256((root / 'Cargo.lock').read_bytes()).hexdigest())
            self.assertNotIn(REVISION, json.dumps(result))
            self.assertNotIn('source_tag', dependency)

    def test_equal_version_registry_migration_reports_registry_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, version='0.1.0')
            dependency = provenance.dependency_provenance(root)['dependencies'][0]
            self.assertEqual(dependency['source'], provenance.REGISTRY)
            self.assertNotIn('revision', dependency)

    def test_git_bootstrap_reports_actual_locked_immutable_revision(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, registry=False, version='0.1.0')
            dependency = provenance.dependency_provenance(root)['dependencies'][0]
            self.assertEqual(dependency['revision'], REVISION)
            self.assertNotIn('release', dependency)
            self.assertNotIn('checksum', dependency)

    def test_git_source_drift_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, registry=False, version='0.1.0', revision='c' * 40)
            with self.assertRaisesRegex(ValueError, 'Git lock identity mismatch'):
                provenance.dependency_provenance(root)

    def test_missing_registry_checksum_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, checksum='')
            with self.assertRaisesRegex(ValueError, 'missing registry archive checksum'):
                provenance.dependency_provenance(root)

    def test_manifest_and_lock_version_drift_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            path = root / 'Cargo.toml'
            path.write_text(path.read_text().replace('=0.2.0', '=0.3.0'))
            with self.assertRaisesRegex(ValueError, 'registry lock identity mismatch'):
                provenance.dependency_provenance(root)

    def test_actual_editor_lock_has_all_six_dependency_coordinates(self):
        root = Path(__file__).resolve().parents[1]
        result = provenance.dependency_provenance(root)
        self.assertEqual(len(result['dependencies']), 6)
        self.assertEqual({record['repository'] for record in result['dependencies']},
                         {'atrinik/renderer', 'atrinik/content-toolkit'})


if __name__ == '__main__':
    unittest.main()
