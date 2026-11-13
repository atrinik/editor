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
    width, height = {'marker': (24, 40), 'stone': (24, 24), 'bush': (28, 24),
                     'crate': (24, 24), 'lamp': (16, 40), 'gate': (16, 32)}.get(name, (32, 32))
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
            elif name == 'bush':
                # A green bush: three overlapping discs on the bottom edge,
                # with a lighter disc on each.
                color = (0, 0, 0, 0)
                for cx, cy, r in ((8.5, 15.5, 7.5), (19.5, 15.5, 7.5), (13.5, 10.5, 8)):
                    if (x - cx) ** 2 + (y - cy) ** 2 <= r * r:
                        color = (52, 112, 58, 255)
                for cx, cy in ((7.5, 13.5), (18.5, 13.5), (12.5, 7.5)):
                    if (x - cx) ** 2 + (y - cy) ** 2 <= 4:
                        color = (98, 164, 92, 255)
            elif name == 'crate':
                # A brown box: dark frame, lighter boards, one diagonal brace.
                color = (126, 84, 46, 255)
                if 3 <= x <= 20 and 3 <= y <= 20:
                    color = (178, 128, 76, 255)
                    if abs(x - y) <= 1:
                        color = (126, 84, 46, 255)
            elif name == 'lamp':
                # A street lamp: a dark post on a wide foot, a pale yellow
                # light in a dark housing at the top.
                color = (0, 0, 0, 0)
                if 6 <= x <= 9 and 10 <= y <= 39:
                    color = (58, 62, 70, 255)
                if 3 <= x <= 12 and 36 <= y <= 39:
                    color = (58, 62, 70, 255)
                if 2 <= x <= 13 and 0 <= y <= 11:
                    color = (58, 62, 70, 255)
                if 4 <= x <= 11 and 2 <= y <= 9:
                    color = (255, 226, 120, 255)
            elif name == 'gate':
                # A narrow gate: two slate posts under a lintel, open
                # between them, with a pale step at the foot.
                color = (0, 0, 0, 0)
                if (x <= 3 or x >= 12) and y >= 4:
                    color = (70, 78, 92, 255)
                if y <= 5:
                    color = (70, 78, 92, 255)
                if 1 <= y <= 3 and 1 <= x <= 14:
                    color = (122, 132, 150, 255)
                if y >= 29 and 4 <= x <= 11:
                    color = (190, 196, 206, 255)
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
    placeable = ['marker', 'stone', 'bush', 'crate', 'lamp']
    # The gate is not placeable: it carries the sample's portals and no
    # editing grant reaches it.
    standing = placeable + ['gate']
    names = ['grass', 'sand'] + placeable + ['grass_edge_' + str(i) for i in range(1, 16)] + ['gate']
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
                   prototypes=[dict(proto=n, resource=n, anchor='standing' if n in standing else 'ground') for n in names],
                   editing=dict(movable_uids=['marker_1'], terrain=[dict(grid='floor', values=['grass', 'sand'])],
                                fields=[dict(uid='marker_1', field='label')],
                                objects=[dict(proto='marker', layer='item', fields=['label']),
                                         dict(proto='stone', layer='item', fields=['label'],
                                              defaults=dict(label='New stone',
                                                            settings=dict(active=False, visits=0)))]
                                + [dict(proto=n, layer='item', fields=['label']) for n in ('bush', 'crate', 'lamp')],
                                zones=dict(paint=True, manage=True),
                                anchors=dict(edit=True)))
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
