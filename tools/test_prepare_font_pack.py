"""Registry binding, notice/SBOM and explicit candidate separation proofs."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from tools import test_font_pack as delivery
from tools import test_package_font_notice as bitmap

spec = importlib.util.spec_from_file_location('prepare_font_pack', Path(__file__).with_name('prepare-font-pack.py'))
prepare = importlib.util.module_from_spec(spec)
with patch.dict(sys.modules, {'font_pack': delivery.pack}):
    spec.loader.exec_module(prepare)


class FontPreparationTests(unittest.TestCase):
    def fixture(self, root):
        editor, crate, stage, metadata, _ = bitmap.PackageFontNoticeTests().fixture(root)
        source, blob, expected = delivery.fixture(root)
        (crate / 'assets/unicode-font-pack.json').write_bytes(blob)
        bitmap.archive_fixture(editor, crate)
        return editor, crate, stage, metadata, source, blob, expected

    def test_registry_staging_and_sbom_are_bound_to_exact_ui_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            root, _, stage, metadata, source, _, expected = self.fixture(Path(directory))
            record = prepare.stage_font_pack(root, source, stage, metadata, expected_sha=expected)
            self.assertEqual(record['binding']['kind'], 'registry')
            self.assertTrue(record['binding']['registry_proof'])
            self.assertEqual(prepare.verify_font_pack(root, stage, metadata, expected), record)
            sbom = stage / 'sbom.cdx.json'
            sbom.write_text(json.dumps({'bomFormat': 'CycloneDX', 'components': [{'type': 'library', 'name': 'binary-dependency'}]}))
            prepare.update_sbom(root, stage, sbom, metadata, update=True, expected_sha=expected)
            prepare.update_sbom(root, stage, sbom, metadata, expected_sha=expected)
            value = json.loads(sbom.read_text())
            value['components'][-1]['hashes'][0]['content'] = '0' * 64
            sbom.write_text(json.dumps(value))
            with self.assertRaisesRegex(ValueError, 'SBOM identity/hash/license'):
                prepare.update_sbom(root, stage, sbom, metadata, expected_sha=expected)

    def test_changed_registry_manifest_and_provenance_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root, crate, stage, metadata, source, blob, expected = self.fixture(Path(directory))
            path = crate / 'assets/unicode-font-pack.json'
            path.write_bytes(blob + b' ')
            with self.assertRaisesRegex(ValueError, 'font registry file checksum mismatch'):
                prepare.stage_font_pack(root, source, stage, metadata, expected_sha=expected)
            path.write_bytes(blob)
            prepare.stage_font_pack(root, source, stage, metadata, expected_sha=expected)
            path = stage / 'font-pack-provenance.json'
            value = json.loads(path.read_text())
            value['binding']['checksum'] = '0' * 64
            path.write_text(json.dumps(value))
            with self.assertRaisesRegex(ValueError, 'registry binding/provenance'):
                prepare.verify_font_pack(root, stage, metadata, expected)

    def test_missing_notice_and_unlisted_staged_file_refused(self):
        for mutation in ('notice', 'extra'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root, _, stage, metadata, source, _, expected = self.fixture(Path(directory))
                prepare.stage_font_pack(root, source, stage, metadata, expected_sha=expected)
                path = stage / 'font-pack/fonts/test/OFL.txt'
                if mutation == 'notice':
                    path.unlink()
                else:
                    (stage / 'font-pack/secret-world.json').write_text('{}')
                with self.assertRaisesRegex(ValueError, 'exact font pack file'):
                    prepare.verify_font_pack(root, stage, metadata, expected)

    def test_candidate_receipt_cannot_pass_production_verification(self):
        with tempfile.TemporaryDirectory() as directory:
            root, crate, stage, metadata, source, _, expected = self.fixture(Path(directory))
            record = prepare.stage_font_pack(root, source, stage, candidate_ui=crate, expected_sha=expected)
            self.assertEqual(record['binding']['kind'], 'local-candidate')
            self.assertFalse(record['binding']['registry_proof'])
            with self.assertRaisesRegex(ValueError, 'registry binding/provenance'):
                prepare.verify_font_pack(root, stage, metadata, expected)


if __name__ == '__main__':
    unittest.main()
