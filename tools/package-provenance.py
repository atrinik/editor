#!/usr/bin/env python3
"""Describe package dependencies using their actual immutable Cargo coordinates."""
# Copyright (c) 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

REGISTRY = 'registry+https://github.com/rust-lang/crates.io-index'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def dependency_provenance(root):
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    lock_bytes = (root / 'Cargo.lock').read_bytes()
    packages = tomllib.loads(lock_bytes.decode())['package']
    policy = json.loads((root / 'policy/released-dependencies.json').read_text())
    require(policy['schema_version'] == 1, 'unsupported dependency policy')
    require(not manifest.get('patch') and not manifest.get('replace'), 'dependency overrides prohibited')
    records = []
    for dep in policy['dependencies']:
        name = dep['name']
        declaration = manifest['workspace']['dependencies'][name]
        matching = [package for package in packages if package['name'] == name]
        require(len(matching) == 1, f'ambiguous or missing locked dependency: {name}')
        package = matching[0]
        version = package['version']
        source = package.get('source')
        record = {'name': name, 'repository': dep['repository'], 'version': version}
        if isinstance(declaration, dict) and 'git' in declaration:
            require(declaration == dep.get('legacy_git'), f'unapproved Git dependency: {name}')
            revision = declaration['rev']
            require(re.fullmatch('[0-9a-f]{40}', revision) is not None, 'Git dependency is not immutable')
            expected = f'git+{declaration["git"]}?rev={revision}#{revision}'
            require(source == expected and version == declaration['version'], f'Git lock identity mismatch: {name}')
            record.update(source=source, revision=revision)
        else:
            if isinstance(declaration, dict):
                require(set(declaration) <= {'version', 'features', 'default-features'}, f'dependency override prohibited: {name}')
                declaration = declaration['version']
            require(declaration == '=' + version and source == REGISTRY, f'registry lock identity mismatch: {name}')
            checksum = package.get('checksum', '')
            require(re.fullmatch('[0-9a-f]{64}', checksum) is not None, f'missing registry archive checksum: {name}')
            record.update(source=source, checksum=checksum)
        records.append(record)
    require(len(records) == len({record['name'] for record in records}), 'duplicate dependency policy identity')
    return {'cargo_lock_sha256': hashlib.sha256(lock_bytes).hexdigest(),
            'dependencies': sorted(records, key=lambda record: record['name'])}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, default=Path('.'))
    parser.add_argument('--version', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    require(re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?', args.version) is not None, 'invalid package version')
    provenance = {'schema_version': 2, 'version': args.version,
                  'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True, timeout=30).strip(),
                  'rust': subprocess.check_output(['rustc', '--version'], text=True, timeout=30).strip(),
                  **dependency_provenance(root)}
    # Refuse an archive when its required font notice was omitted or altered.
    provenance.update(json.loads(subprocess.check_output(
        [sys.executable, str(Path(__file__).with_name('package-font-notice.py')),
         '--root', str(root), '--stage', str(args.output.parent.resolve()), '--verify'],
        text=True, timeout=150)))
    provenance.update(json.loads(subprocess.check_output(
        [sys.executable, str(Path(__file__).with_name('prepare-font-pack.py')), 'verify',
         '--root', str(root), '--stage', str(args.output.parent.resolve()),
         '--sbom', str(args.output.parent.resolve() / 'sbom.cdx.json')],
        text=True, timeout=150)))
    args.output.write_text(json.dumps(provenance, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, subprocess.SubprocessError) as error:
        raise SystemExit(f'package provenance failed: {error}') from error
