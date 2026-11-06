#!/usr/bin/env python3
"""Explicit font preparation/staging bound to immutable renderer crate metadata.

Production stage/verify/sbom modes never fetch. Only prepare --acquire performs
public pinned, bounded, credential-free downloads. Candidate mode never claims
registry proof and is refused by the production verifier.
"""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import font_pack as pack

SCRIPT = Path(__file__).with_name('package-font-notice.py')
spec = importlib.util.spec_from_file_location('bitmap_font_notice', SCRIPT)
notice = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notice)


def trusted_manifest(root, metadata=None, candidate_ui=None, expected_sha=pack.MANIFEST_SHA256):
    if candidate_ui is not None:
        pack.require(candidate_ui.is_absolute() and candidate_ui.resolve() == candidate_ui,
                     'candidate UI crate must be explicit absolute path without links')
        blob = pack.read_regular(candidate_ui / 'assets/unicode-font-pack.json', pack.MAX_MANIFEST)
        binding = {'kind': 'local-candidate', 'crate': str(candidate_ui), 'registry_proof': False}
    else:
        if metadata is None:
            metadata = json.loads(subprocess.check_output(
                ['cargo', 'metadata', '--locked', '--offline', '--format-version', '1'],
                cwd=root, text=True, timeout=120))
        verified = notice.verified_font(root, metadata)
        pack.require(verified is not None, 'Unicode font delivery requires the released UI crate')
        _, registry = verified
        entry = next(entry for entry in metadata['packages'] if entry['name'] == notice.NAME)
        crate = Path(entry['manifest_path']).parent
        blob = pack.read_regular(crate / 'assets/unicode-font-pack.json', pack.MAX_MANIFEST)
        notice.registry_files(crate, registry['checksum'], ['assets/unicode-font-pack.json'])
        binding = {'kind': 'registry', 'name': registry['package'], 'version': registry['version'],
                   'source': registry['source'], 'checksum': registry['checksum'], 'registry_proof': True}
    pack.load_manifest(blob, expected_sha)
    return blob, binding


def stage_font_pack(root, source, stage, metadata=None, candidate_ui=None, expected_sha=pack.MANIFEST_SHA256):
    blob, binding = trusted_manifest(root, metadata, candidate_ui, expected_sha)
    record = {'schema_version': 1, 'binding': binding,
              **pack.copy_pack(source, stage / 'font-pack', blob, expected_sha)}
    with (stage / 'font-pack-provenance.json').open('x') as output:
        json.dump(record, output, indent=2, sort_keys=True)
        output.write('\n')
    return record


def verify_font_pack(root, stage, metadata=None, expected_sha=pack.MANIFEST_SHA256):
    blob, binding = trusted_manifest(root, metadata, expected_sha=expected_sha)
    record = {'schema_version': 1, 'binding': binding, **pack.verify(stage / 'font-pack', blob, expected_sha)}
    staged = json.loads(pack.read_regular(stage / 'font-pack-provenance.json', 128 * 1024))
    pack.require(staged == record, 'font pack registry binding/provenance mismatch')
    return record


def update_sbom(root, stage, sbom_path, metadata=None, update=False, expected_sha=pack.MANIFEST_SHA256):
    record = verify_font_pack(root, stage, metadata, expected_sha)
    sbom = json.loads(pack.read_regular(sbom_path, 32 * 1024 * 1024))
    pack.require(sbom['bomFormat'] == 'CycloneDX' and isinstance(sbom['components'], list),
                 'font pack requires the binary CycloneDX SBOM')
    expected = pack.components(record)
    found = [component for component in sbom['components']
             if str(component.get('bom-ref', '')).startswith('font-pack:')]
    if update:
        pack.require(not found, 'font pack SBOM components already present')
        sbom['components'].extend(expected)
        # The package stage is private. Refuse a link/special target before the
        # write, as for all staged package inputs; final verification follows.
        with sbom_path.open('w', encoding='utf-8', newline='\n') as output:
            json.dump(sbom, output, sort_keys=True, separators=(',', ':'))
            output.write('\n')
    else:
        pack.require(found == expected, 'font pack SBOM identity/hash/license mismatch')
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('prepare', 'stage', 'candidate', 'verify', 'sbom'))
    parser.add_argument('--root', type=Path, default=Path('.'))
    parser.add_argument('--font-pack', type=Path, help='explicit previously prepared pack; no implicit downloads')
    parser.add_argument('--stage', type=Path)
    parser.add_argument('--output', type=Path, help='new absent prepared pack directory')
    parser.add_argument('--candidate-ui', type=Path, help='explicit local crate; prepare/candidate modes only')
    parser.add_argument('--acquire', action='store_true', help='opt in to bounded public font preparation')
    parser.add_argument('--sbom', type=Path)
    args = parser.parse_args()
    root = args.root.resolve(strict=True)
    pack.require(args.mode in ('prepare', 'candidate') or args.candidate_ui is None,
                 'production font delivery refuses local candidate metadata')
    pack.require(args.mode == 'prepare' or not args.acquire, 'downloads allowed only in explicit prepare mode')
    if args.mode == 'prepare':
        pack.require(args.acquire and args.output is not None, 'prepare requires --acquire and a new --output')
        blob, binding = trusted_manifest(root, candidate_ui=args.candidate_ui)
        destination = args.output.absolute()
        record = {'schema_version': 1, 'binding': binding, **pack.acquire(destination, blob)}
    else:
        pack.require(args.stage is not None, 'font delivery requires --stage')
        stage = args.stage.absolute()
        pack.require(stage.resolve() == stage and stage.is_dir(), 'font stage must exist without links')
        if args.mode in ('stage', 'candidate'):
            pack.require(args.font_pack is not None, 'font staging requires explicit --font-pack; run prepare separately')
            pack.require(args.mode != 'candidate' or args.candidate_ui is not None,
                         'candidate mode requires explicit --candidate-ui')
            record = stage_font_pack(root, args.font_pack.absolute(), stage,
                                     candidate_ui=args.candidate_ui)
        elif args.mode == 'sbom':
            pack.require(args.sbom is not None, 'SBOM update requires --sbom')
            record = update_sbom(root, stage, args.sbom.absolute(), update=True)
        elif args.sbom is not None:
            record = update_sbom(root, stage, args.sbom.absolute())
        else:
            record = verify_font_pack(root, stage)
    print(json.dumps({'unicode_font_pack': record}, sort_keys=True))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, notice.tarfile.TarError, subprocess.SubprocessError) as error:
        raise SystemExit(f'Unicode font delivery refused: {error}') from error
