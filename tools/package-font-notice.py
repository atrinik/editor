#!/usr/bin/env python3
"""Carry the exact bundled UI font notice from verified registry crate assets."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import argparse
import hashlib
import json
import io
from pathlib import Path
import subprocess
import tarfile
import tomllib

REGISTRY = 'registry+https://github.com/rust-lang/crates.io-index'
NAME = 'tilewright-render-ui'
VERSION = '0.6.2'
LICENSE = 'MIT AND OFL-1.1'
DESTINATION = 'licenses/OFL-RobotoMono.txt'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(blob):
    return hashlib.sha256(blob).hexdigest()


def regular_bytes(path):
    require(not path.is_symlink() and path.is_file() and path.resolve() == path,
            f'font asset must be a regular file without links: {path.name}')
    return path.read_bytes()


def registry_files(crate, checksum, names):
    """Authenticate actual Cargo cache bytes, without trusting extracted metadata."""
    require(crate.parent.parent.name == 'src' and crate.parent.parent.parent.name == 'registry'
            and crate.name == f'{NAME}-{VERSION}', 'unexpected UI registry cache location')
    archive = crate.parent.parent.parent / 'cache' / crate.parent.name / (crate.name + '.crate')
    require(archive.stat().st_size <= 8 * 1024 * 1024, 'UI registry archive too large')
    blob = regular_bytes(archive)
    require(digest(blob) == checksum, 'UI font registry archive checksum mismatch')
    wanted = {f'{crate.name}/{name}': name for name in names}
    found = {}
    seen = set()
    size = 0
    with tarfile.open(fileobj=io.BytesIO(blob), mode='r:gz') as bundle:
        for member in bundle:
            parts = member.name.split('/')
            require(len(seen) < 4096 and member.name not in seen and parts[0] == crate.name
                    and all(part not in ('', '.', '..') for part in parts)
                    and (member.isfile() or member.isdir()), 'unsafe UI registry archive member')
            seen.add(member.name)
            size += member.size
            require(0 <= member.size <= 16 * 1024 * 1024 and size <= 32 * 1024 * 1024,
                    'UI registry archive contents too large')
            if member.name in wanted:
                require(member.isfile(), 'UI registry asset must be a regular file')
                found[wanted[member.name]] = bundle.extractfile(member).read()
    require(set(found) == set(names), 'UI registry archive missing required asset')
    for name, data in found.items():
        require(regular_bytes(crate / name) == data, f'font registry file checksum mismatch: {name}')
    return found


def verified_font(root, metadata=None):
    """Return a verified notice/record, or None for releases without the UI crate.

    No overrides, downloads, font discovery, parsing or rasterization occur here.
    Cargo has already acquired and checksum-verified the immutable registry crate;
    verify its unpacked assets against both Cargo and this exact asset policy.
    """
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    require(not manifest.get('patch') and not manifest.get('replace'),
            'dependency overrides prohibited for font packaging')
    locked = [package for package in tomllib.loads((root / 'Cargo.lock').read_text())['package']
              if package['name'] == NAME]
    if not locked:
        return None
    require(len(locked) == 1, 'ambiguous UI font dependency')
    package = locked[0]
    require(package.get('source') == REGISTRY and package['version'] == VERSION,
            'UI font requires the reviewed immutable registry package/version')
    checksum = package.get('checksum', '')
    require(len(checksum) == 64 and all(char in '0123456789abcdef' for char in checksum),
            'UI font registry archive checksum missing')
    declaration = manifest['workspace']['dependencies'][NAME]
    if isinstance(declaration, dict):
        require(set(declaration) <= {'version', 'features', 'default-features'},
                'UI font dependency override prohibited')
        declaration = declaration['version']
    require(declaration == '=' + VERSION, 'UI font manifest must pin the exact version')
    policy = json.loads((root / 'policy/font-notice.json').read_text())
    require(set(policy) == {'schema_version', 'package', 'manifest_sha256', 'source_sha256',
                            'notice_sha256', 'notice_destination'} and policy['schema_version'] == 1,
            'unsupported font notice policy')
    require(policy['package'] == {'name': NAME, 'version': VERSION, 'license': LICENSE}
            and policy['notice_destination'] == DESTINATION, 'font package policy identity')
    if metadata is None:
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--locked', '--offline', '--format-version', '1'],
            cwd=root, text=True, timeout=120))
    matches = [entry for entry in metadata['packages'] if entry['name'] == NAME]
    require(len(matches) == 1, 'ambiguous or missing UI font metadata')
    entry = matches[0]
    require(entry['version'] == VERSION and entry.get('source') == REGISTRY
            and entry.get('license') == LICENSE, 'UI font Cargo metadata identity/license mismatch')
    crate = Path(entry['manifest_path']).parent
    require(crate.is_absolute() and crate.resolve() == crate, 'UI font source contains a link')
    # The policy pins the whole producer manifest. Its per-file digests identify
    # generated font data; the producer checker owns cmap/metrics validation.
    manifest_blob = regular_bytes(crate / 'assets/manifest.json')
    require(digest(manifest_blob) == policy['manifest_sha256'], 'font asset manifest checksum mismatch')
    font = json.loads(manifest_blob)
    require(font['schema'] == 1 and font['license'] == 'OFL-1.1', 'font manifest license/schema')
    blobs = {'manifest.json': manifest_blob}
    for name in ['OFL.txt', 'RobotoMono-Regular.ttf', *font['files']]:
        require(Path(name).name == name, 'font asset path outside package assets')
        blobs[name] = regular_bytes(crate / 'assets' / name)
    registry_files(crate, checksum, ['assets/' + name for name in blobs])
    require(digest(blobs['OFL.txt']) == policy['notice_sha256'] == font['notice']['sha256'],
            'font notice checksum mismatch')
    require(digest(blobs['RobotoMono-Regular.ttf']) == policy['source_sha256'] == font['source']['sha256'],
            'font source checksum mismatch')
    for name, blob in blobs.items():
        if name in font['files']:
            record = font['files'][name]
            require(record['sha256'] == digest(blob) and record['bytes'] == len(blob),
                    f'generated font asset checksum mismatch: {name}')
    record = {'package': NAME, 'version': VERSION, 'source': REGISTRY, 'checksum': checksum,
              'license': 'OFL-1.1', 'upstream': font['source'], 'copyright': font['copyright'],
              'manifest_sha256': digest(manifest_blob),
              'notice': {'path': DESTINATION, 'sha256': digest(blobs['OFL.txt']),
                         'upstream': font['notice']['url']},
              'embedded_assets': font['files']}
    return blobs['OFL.txt'], record


def package_font(root, stage, metadata=None, verify=False):
    font = verified_font(root, metadata)
    if font is None:
        require(not (stage / DESTINATION).exists() and not (stage / DESTINATION).is_symlink(),
                'unexpected font notice without a UI dependency')
        return {'font_assets': []}
    notice, record = font
    target = stage / DESTINATION
    if verify:
        require(regular_bytes(target) == notice, 'packaged UI font notice mismatch')
    else:
        target.parent.mkdir(parents=True, exist_ok=True)
        require(target.parent.resolve() == target.parent, 'font notice destination contains a link')
        with target.open('xb') as output:
            output.write(notice)
    return {'font_assets': [record]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('.'))
    parser.add_argument('--stage', type=Path, required=True)
    parser.add_argument('--verify', action='store_true')
    args = parser.parse_args()
    print(json.dumps(package_font(args.root.resolve(strict=True), args.stage.resolve(strict=True),
                                  verify=args.verify), sort_keys=True))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError, subprocess.SubprocessError) as error:
        raise SystemExit(f'package font refused: {error}') from error
