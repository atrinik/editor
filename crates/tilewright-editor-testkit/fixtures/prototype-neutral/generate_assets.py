#!/usr/bin/env python3
# Copyright 2026 Seraphina Studios
# SPDX-License-Identifier: MIT
"""Reproduce original geometric fixture pixels and verify their exact provenance."""
import argparse
import hashlib
import json
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[3]
PREFIX = ROOT.relative_to(REPO).as_posix()
AUTHOR = 'Seraphina Studios'
NOTICE = 'Copyright 2026 Seraphina Studios; original geometric fixture, MIT.'


def image(name):
    width, height = {'marker': (24, 40), 'stone': (24, 24)}.get(name, (32, 32))
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            if name == 'marker':
                color = (0, 0, 0, 0)
                if 10 <= x <= 13 and 12 <= y <= 37:
                    color = (105, 63, 46, 255)
                if 2 <= x <= 21 and 3 <= y <= 17:
                    color = (43, 67, 102, 255)
                if 4 <= x <= 19 and 5 <= y <= 15:
                    color = (245, 178, 66, 255)
                if 8 <= x <= 15 and 8 <= y <= 12:
                    color = (255, 239, 193, 255)
            elif name == 'stone':
                # A grey rounded stone: a 22x18 ellipse resting on the
                # bottom edge, with a lighter upper face.
                color = (0, 0, 0, 0)
                dx, dy = (x - 11.5) / 11, (y - 14.5) / 9
                if dx * dx + dy * dy <= 1:
                    color = (96, 101, 110, 255)
                    ex, ey = (x - 10.5) / 7, (y - 11.5) / 4
                    if ex * ex + ey * ey <= 1:
                        color = (150, 156, 166, 255)
            elif name.startswith('grass_edge_'):
                mask = int(name.rsplit('_', 1)[1])
                edge = ((mask & 1 and y < 4) or (mask & 2 and x >= 28)
                        or (mask & 4 and y >= 28) or (mask & 8 and x < 4))
                color = (72, 126, 69, 255) if edge else (0, 0, 0, 0)
            elif name == 'grass':
                color = (91, 145, 77, 255)
                if (x * 7 + y * 11) % 47 < 3:
                    color = (111, 165, 87, 255)
            else:
                color = (204, 179, 116, 255)
                if (x * 13 + y * 3) % 53 < 3:
                    color = (226, 204, 143, 255)
            pixels.extend(color)
    return width, height, b'ATX1' + struct.pack('<II', width, height) + pixels


def outputs():
    resources, assets = [], []
    names = ['grass', 'sand', 'marker', 'stone'] + ['grass_edge_' + str(i) for i in range(1, 16)]
    result = {}
    for name in names:
        width, height, data = image(name)
        path = 'preview-assets/' + name + '.atx1'
        sha = hashlib.sha256(data).hexdigest()
        result[ROOT / path] = data
        resources.append(dict(id=name, path=path, sha256=sha, width=width, height=height,
                              license='MIT', author=AUTHOR, notice=NOTICE))
        assets.append(dict(path=PREFIX + '/' + path, source=PREFIX + '/generate_assets.py',
                           author=AUTHOR, license='MIT', sha256=sha, width=width, height=height,
                           transformation='Deterministic authored integer pixel geometry; standalone ATX1 RGBA8.',
                           notice=NOTICE))
    catalog = dict(format='tilewright.editor-preview/1', profile=dict(id='sample.meadow', version='1'),
                   view=dict(plane='main', layer=0, bounds=[0, 0, 8, 6]), presentation='authored',
                   resources=resources,
                   prototypes=[dict(proto=n, resource=n, anchor='standing' if n in ('marker', 'stone') else 'ground') for n in names],
                   editing=dict(movable_uids=['marker_1'], terrain=[dict(grid='floor', values=['grass', 'sand'])],
                                fields=[dict(uid='marker_1', field='label')],
                                objects=[dict(proto='marker', layer='item', fields=['label']),
                                         dict(proto='stone', layer='item', fields=['label'],
                                              defaults=dict(label='New stone',
                                                            settings=dict(active=False, visits=0)))]))
    result[ROOT / 'preview.jsonc'] = (json.dumps(catalog, indent=2) + '\n').encode()
    result[REPO / 'provenance/assets.json'] = (json.dumps(dict(schema_version=1, assets=assets), indent=2) + '\n').encode()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true', help='replace fixture assets and manifests with reproduced bytes')
    args = parser.parse_args()
    for path, expected in outputs().items():
        if args.write:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(expected)
        elif path.read_bytes() != expected:
            raise SystemExit('Reproduction/provenance mismatch: ' + str(path.relative_to(REPO)))
    print('Authored assets, ATX1 dimensions/pixels, catalog and provenance reproduce exactly.')


if __name__ == '__main__':
    main()
