# Third-party notices

The editor source is MIT. Its exact dependency graph is recorded by
`Cargo.lock`, checked by `cargo-deny`, and included in each CycloneDX release
SBOM.

- `tilewright-source`, `tilewright-edit` and `tilewright-jsonc` 0.2.0
  (tilewright-world): MIT.
- `tilewright-scene` and `tilewright-render-*` 0.6.3 (tilewright-renderer): MIT
  implementation; `tilewright-render-ui` also bundles OFL-1.1 font data.
- `sdl3` Rust bindings: MIT.
- SDL native library: Zlib License.
- `libloading` (native graphics loading): ISC License; its notice remains in
  the dependency source and generated SBOM.
- `unicode-ident` (Rust macro identifier tables): MIT/Apache-2.0 and Unicode
  License v3; its Unicode data notice remains in the dependency source.
- `wgpu` and graphics/shader support: MIT or Apache-2.0, with exact versions
  and selected alternatives recorded by the dependency license check/SBOM.
- Shared renderer text layout: `parley`, `parley_engine`, `parley_data`,
  `fontique` 0.12.0, `parlance` 0.1.1, `swash` 0.2.10, `skrifa` 0.44.0,
  `read-fonts` 0.41.0 and `font-types` 0.12.7: MIT or Apache-2.0;
  `parley_emoji` 0.12.0 and `harfrust` 0.12.0: MIT. `fontique` is built
  without its `system` feature, so no system font discovery is linked.
- ICU4X line breaking, segmentation and character properties: `icu_*` 2.4.0
  (including their compiled `*_data` crates), `zerovec`, `zerotrie`,
  `zerofrom`, `yoke`, `litemap`, `tinystr`, `writeable` and `potential_utf`:
  Unicode License v3; the notice is reproduced below. Exact dependency
  licenses are checked against the locked graph.
- `png`, compression support, SHA-256 and JSON encoding: MIT-compatible
  alternatives recorded by the exact locked dependency graph.

The bundled `samples/prototype-neutral` data and standalone ATX1 resources are
original MIT geometric fixtures by Seraphina Studios. Exact source, author,
license, transformation and SHA-256 records are in `provenance/assets.json`.
No external game map or original game artwork is included.

Authored content opened by the editor is not part of this software and retains
its own license and attribution.

## libloading 0.8.9 — ISC notice

Copyright © 2015, Simonas Kazlauskas

Permission to use, copy, modify, and/or distribute this software for any purpose with or without
fee is hereby granted, provided that the above copyright notice and this permission notice appear
in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS
SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE
AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
THIS SOFTWARE.

## unicode-ident 1.0.27 — Unicode data notice

UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2023 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.

## ICU4X 2.4.0 — Unicode data notice

UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 2020-2024 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.

SPDX-License-Identifier: Unicode-3.0

—

Portions of ICU4X may have been adapted from ICU4C and/or ICU4J.
ICU 1.8.1 to ICU 57.1 © 1995-2016 International Business Machines Corporation and others.

## Shared renderer UI font — OFL 1.1

`tilewright-render-ui` 0.6.3 combines MIT implementation with bundled Roboto Mono
font source and derived atlas, metrics and glyph data under **SIL Open Font
License 1.1 (`OFL-1.1`)**. The exact released registry archive is pinned in
`Cargo.lock`; its checksum and source/notice hashes are recorded in
`PROVENANCE.md`. Font attribution does not establish text-coverage acceptance.

Copyright 2015 The Roboto Mono Project Authors
(https://github.com/googlefonts/robotomono).

Source: `googlefonts/RobotoMono`, revision
`895ec691990d041dd727c7b5afa3ce56525d98e6`,
[`fonts/ttf/RobotoMono-Regular.ttf`](https://github.com/googlefonts/RobotoMono/blob/895ec691990d041dd727c7b5afa3ce56525d98e6/fonts/ttf/RobotoMono-Regular.ttf).
The shared renderer converts selected common-Latin/symbol glyphs to 20 px
bitmap/metric data; the editor embeds the immutable resources without further
font transformation. The synthetic solid texture and renderer/editor code
remain MIT.

The complete, unmodified upstream copyright and OFL 1.1 license are retained as
`assets/OFL.txt` in the UI crate and copied to
`licenses/OFL-RobotoMono.txt` in binary archives containing it. No Reserved Font
Names are declared in that notice. Preserve this notice with the font data.
`PROVENANCE.md` and `policy/font-notice.json` record the exact immutable source,
notice and manifest hashes; package provenance records the verified registry
crate and generated assets.

## Separate Unicode Noto source font pack (OFL-1.1)

The unmodified source font files in the packaged `font-pack/fonts/` tree remain under SIL Open
Font License 1.1. Their exact original licenses are retained at each family's
`OFL.txt`; `font-pack/manifest.json` records every original source/notice
path, author, copyright, digest and embedded name/license record from pinned
`google/fonts` revision `6c1dbe27dcf7d817d4b62cec033ba77fa8e2a5ec`.

Noto Sans, Noto Sans Arabic/Hebrew/Devanagari/Bengali/Gujarati/Gurmukhi/Tamil/
Telugu/Kannada/Malayalam/Oriya/Sinhala/Thai: Copyright 2022 The Noto Project
Authors, with the original per-family upstream project URLs in their notices.
No Reserved Font Names are declared in those notices.

Noto Sans SC and KR: Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved
Font Name 'Source'.

Noto Emoji: Copyright 2013 Google LLC. No Reserved Font Names are declared.

These fonts have no transformations. Their original names, embedded copyright
records and full notices are retained. OFL font data are not relicensed MIT.
Redistribution/embedding must retain the relevant copyright and full OFL notice;
see the exact per-family `OFL.txt` files. The renderer source checker and the independently authored editor delivery
helpers are MIT. The exact manifests/font inventories remain OFL-1.1.
