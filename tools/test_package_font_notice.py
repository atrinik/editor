"""Refusal proofs for immutable UI font attribution in binary packages."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import hashlib
import importlib.util
import json
import io
from pathlib import Path
import tempfile
import tarfile
import tomllib
import unittest

spec = importlib.util.spec_from_file_location('package_font_notice', Path(__file__).with_name('package-font-notice.py'))
font = importlib.util.module_from_spec(spec)
spec.loader.exec_module(font)


def sha(blob):
    return hashlib.sha256(blob).hexdigest()


def archive_fixture(root, crate, extra=None):
    archive = crate.parents[2] / 'cache' / crate.parent.name / (crate.name + '.crate')
    archive.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, 'w:gz') as bundle:
        for path in sorted(crate.rglob('*')):
            if path.is_file():
                blob = path.read_bytes()
                member = tarfile.TarInfo(crate.name + '/' + str(path.relative_to(crate)))
                member.size = len(blob)
                bundle.addfile(member, io.BytesIO(blob))
        if extra:
            bundle.addfile(extra, io.BytesIO(b'x' * extra.size))
    (root / 'Cargo.lock').write_text(f'[[package]]\nname = "{font.NAME}"\nversion = "{font.VERSION}"\nsource = "{font.REGISTRY}"\nchecksum = "{sha(archive.read_bytes())}"\n')
    return archive


class PackageFontNoticeTests(unittest.TestCase):
    def fixture(self, base):
        root, stage = base / 'editor', base / 'stage'
        crate = base / 'registry/src/index.crates.io-test' / f'{font.NAME}-{font.VERSION}'
        for path in (root / 'policy', crate / 'assets', stage):
            path.mkdir(parents=True)
        (root / 'Cargo.toml').write_text('[workspace.dependencies]\ntilewright-render-ui = "=0.6.2"\n')
        # Original synthetic test bytes: no external font is copied into tests.
        blobs = {'OFL.txt': b'synthetic notice bytes for packaging test\n',
                 'RobotoMono-Regular.ttf': b'synthetic source bytes',
                 'atlas.atx': b'synthetic atlas bytes'}
        manifest = {'schema': 1, 'license': 'OFL-1.1', 'copyright': 'synthetic fixture',
                    'source': {'sha256': sha(blobs['RobotoMono-Regular.ttf']), 'revision': 'test-fixture'},
                    'notice': {'sha256': sha(blobs['OFL.txt']), 'url': 'https://example.invalid/pinned-notice'},
                    'files': {'atlas.atx': {'sha256': sha(blobs['atlas.atx']), 'bytes': len(blobs['atlas.atx']), 'license': 'OFL-1.1'}}}
        blobs['manifest.json'] = json.dumps(manifest).encode()
        for name, blob in blobs.items():
            (crate / 'assets' / name).write_bytes(blob)
        archive_fixture(root, crate)
        policy = {'schema_version': 1, 'package': {'name': font.NAME, 'version': font.VERSION, 'license': font.LICENSE},
                  'manifest_sha256': sha(blobs['manifest.json']), 'source_sha256': manifest['source']['sha256'],
                  'notice_sha256': manifest['notice']['sha256'], 'notice_destination': font.DESTINATION}
        (root / 'policy/font-notice.json').write_text(json.dumps(policy))
        metadata = {'packages': [{'name': font.NAME, 'version': font.VERSION, 'source': font.REGISTRY,
                    'license': font.LICENSE, 'manifest_path': str(crate / 'Cargo.toml')}]}
        return root, crate, stage, metadata, blobs

    def test_exact_notice_copied_and_verified_with_immutable_provenance(self):
        with tempfile.TemporaryDirectory() as directory:
            root, _, stage, metadata, blobs = self.fixture(Path(directory))
            result = font.package_font(root, stage, metadata)
            self.assertEqual((stage / font.DESTINATION).read_bytes(), blobs['OFL.txt'])
            self.assertEqual(font.package_font(root, stage, metadata, verify=True), result)
            record = result['font_assets'][0]
            self.assertEqual(record['checksum'], tomllib.loads((root / 'Cargo.lock').read_text())['package'][0]['checksum'])
            self.assertEqual(record['source'], font.REGISTRY)
            self.assertEqual(record['notice']['sha256'], sha(blobs['OFL.txt']))
            self.assertEqual(record['embedded_assets']['atlas.atx']['sha256'], sha(blobs['atlas.atx']))

    def test_missing_or_altered_packaged_notice_refused(self):
        for altered in (False, True):
            with self.subTest(altered=altered), tempfile.TemporaryDirectory() as directory:
                root, _, stage, metadata, _ = self.fixture(Path(directory))
                if altered:
                    font.package_font(root, stage, metadata)
                    (stage / font.DESTINATION).write_bytes(b'wrong notice')
                with self.assertRaisesRegex(ValueError, 'regular file|notice mismatch'):
                    font.package_font(root, stage, metadata, verify=True)

    def test_source_notice_manifest_and_atlas_tampering_refused(self):
        for name in ('OFL.txt', 'RobotoMono-Regular.ttf', 'manifest.json', 'atlas.atx'):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root, crate, stage, metadata, _ = self.fixture(Path(directory))
                with (crate / 'assets' / name).open('ab') as output:
                    output.write(b'changed')
                # A forged extraction-side checksum file cannot authorize new bytes.
                (crate / '.cargo-checksum.json').write_text(json.dumps({'files': {
                    'assets/' + name: sha((crate / 'assets' / name).read_bytes())}}))
                with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                    font.package_font(root, stage, metadata)
                self.assertFalse((stage / font.DESTINATION).exists())

    def test_registry_archive_checksum_mismatch_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root, crate, stage, metadata, _ = self.fixture(Path(directory))
            archive = crate.parents[2] / 'cache' / crate.parent.name / (crate.name + '.crate')
            archive.write_bytes(archive.read_bytes() + b'tampered')
            with self.assertRaisesRegex(ValueError, 'archive checksum mismatch'):
                font.package_font(root, stage, metadata)

    def test_missing_archive_and_forged_extracted_checksums_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root, crate, stage, metadata, _ = self.fixture(Path(directory))
            (crate / '.cargo-checksum.json').write_text('{}')
            archive = crate.parents[2] / 'cache' / crate.parent.name / (crate.name + '.crate')
            archive.unlink()
            with self.assertRaises(OSError):
                font.package_font(root, stage, metadata)

    def test_unsafe_or_duplicate_archive_members_refused(self):
        for name, kind in (('../escape', tarfile.REGTYPE),
                           (f'{font.NAME}-{font.VERSION}/assets/OFL.txt', tarfile.REGTYPE),
                           (f'{font.NAME}-{font.VERSION}/link', tarfile.SYMTYPE)):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root, crate, stage, metadata, _ = self.fixture(Path(directory))
                member = tarfile.TarInfo(name)
                member.type = kind
                archive_fixture(root, crate, member)
                with self.assertRaisesRegex(ValueError, 'unsafe UI registry archive'):
                    font.package_font(root, stage, metadata)

    def test_unreviewed_version_local_override_and_license_drift_refused(self):
        for key, value in (('version', '0.6.3'), ('source', None), ('license', 'MIT')):
            with self.subTest(key=key), tempfile.TemporaryDirectory() as directory:
                root, _, stage, metadata, _ = self.fixture(Path(directory))
                metadata['packages'][0][key] = value
                with self.assertRaisesRegex(ValueError, 'identity/license mismatch'):
                    font.package_font(root, stage, metadata)

    def test_lock_version_source_and_manifest_override_refused(self):
        for old, new in (('0.6.2', '0.6.3'), (font.REGISTRY, 'path+file:///local-ui')):
            with self.subTest(new=new), tempfile.TemporaryDirectory() as directory:
                root, _, stage, metadata, _ = self.fixture(Path(directory))
                lock = root / 'Cargo.lock'
                lock.write_text(lock.read_text().replace(old, new))
                with self.assertRaisesRegex(ValueError, 'immutable registry package/version'):
                    font.package_font(root, stage, metadata)
        with tempfile.TemporaryDirectory() as directory:
            root, _, stage, metadata, _ = self.fixture(Path(directory))
            path = root / 'Cargo.toml'
            path.write_text(path.read_text() + '\n[patch.crates-io]\ntilewright-render-ui = { path = "../local-ui" }\n')
            with self.assertRaisesRegex(ValueError, 'overrides prohibited'):
                font.package_font(root, stage, metadata)

    def test_links_and_existing_destination_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root, crate, stage, metadata, _ = self.fixture(Path(directory))
            notice = crate / 'assets/OFL.txt'
            notice.rename(crate / 'elsewhere.txt')
            notice.symlink_to(crate / 'elsewhere.txt')
            with self.assertRaisesRegex(ValueError, 'regular file'):
                font.package_font(root, stage, metadata)
        with tempfile.TemporaryDirectory() as directory:
            root, _, stage, metadata, _ = self.fixture(Path(directory))
            font.package_font(root, stage, metadata)
            with self.assertRaises(FileExistsError):
                font.package_font(root, stage, metadata)

    def test_old_release_without_ui_requires_no_font_notice(self):
        with tempfile.TemporaryDirectory() as directory:
            root, _, stage, _, _ = self.fixture(Path(directory))
            (root / 'Cargo.lock').write_text('package = []\n')
            self.assertEqual(font.package_font(root, stage), {'font_assets': []})

    def test_deny_exception_is_exact_and_not_globally_allowed(self):
        root = Path(__file__).resolve().parents[1]
        licenses = tomllib.loads((root / 'deny.toml').read_text())['licenses']
        self.assertNotIn('OFL-1.1', licenses['allow'])
        self.assertEqual(licenses['exceptions'], [{'crate': 'tilewright-render-ui@0.6.2', 'allow': ['OFL-1.1']}])
        self.assertEqual(json.loads((root / 'policy/font-notice.json').read_text())['package'],
                         {'name': font.NAME, 'version': font.VERSION, 'license': font.LICENSE})


if __name__ == '__main__':
    unittest.main()
