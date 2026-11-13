# Provenance

All implementation, tests, documentation, and synthetic fixtures in this
repository are MIT work by Seraphina Studios. No outside implementation, tests,
configuration, or fixtures are copied or adapted. Dependency font data retains
its own license as recorded below.

Independent implementation remains the default. Before any outside source is
inspected as implementation reference or reused, confirm that its license is
compatible with MIT and record the exact source, revision, destination,
transformation, and notice here. GPL/AGPL material is never copied, adapted, or
used as a build or runtime dependency.

`provenance/reuse.json` records reused material (none), and
`provenance/assets.json` is the exact authored neutral-sample asset allowlist.
Every listed standalone ATX1 resource records its generator source, author,
MIT license, digest, dimensions, transformation and notice. Foundation checks
verify the payload and reject unlisted resources or paths outside the sample.
The package data allowlist contains only this original neutral World fixture;
external imported game maps/art, recovery and task state are excluded.

Authored project content retains its individual license. Opening, previewing,
editing, validating, or packaging it does not apply this repository's MIT
license. Attribution and unknown licensing fields remain lossless data.

Dependency notice text in `THIRD_PARTY_NOTICES.md` is retained verbatim from
`libloading` 0.8.9 `LICENSE` (ISC; crates.io archive SHA-256
`d7c4b02199fee7c5d21a5ae7d8cfa79a6ef5bb2fc834d6e9058e89c825efdc55`)
`unicode-ident` 1.0.27 `LICENSE-UNICODE` (Unicode-3.0; archive SHA-256
`a2c754d6c33795a1c324727428e5a7dedb5b06195f9890bdbcba760d3e246563`)
and `icu_segmenter` 2.4.0 `LICENSE` (Unicode-3.0; archive SHA-256
`61b195dc605f81aa181bec9b3bd14e26cc71b903e4644f6af578ca9772c7a710`;
file SHA-256 `f367c1b8e1aa262435251e442901da4607b4650e0e63a026f5044473ecfb90f2`,
identical in every locked ICU4X 2.4.0 and support crate that the renderer's
Parley text layout brings in).
Transformation: verbatim license notice inclusion for binary/source package
attribution; no dependency implementation is copied into editor source.

## Shared renderer UI font

The package policy and production Cargo graph pin `tilewright-render-ui`
**0.7.1** from crates.io, archive SHA-256
`4ca52c72820cd74458128ba651250b81b33ddd548a94b755c2c385258c12392c`.
All eight renderer 0.7.1 registry archives match the immutable renderer
[v1.27.0 release](https://github.com/seraphinastudios/tilewright-renderer/releases/tag/v1.27.0),
source revision `d2a767c602be942b6d8964ae238d3618fad294f5`, release manifest
SHA-256 `b1d4c14e6e6c9df5e64f0350c2cf5ddb8a2092ebca3760384d57c70b34770bcc`.
Its bundled font, atlas, metrics and Unicode font-pack metadata are
byte-identical to 0.7.0, so the font notice policy hashes are unchanged.
This attribution and dependency migration do not claim text-coverage acceptance.

The shared renderer implementation is MIT. Its bundled Roboto Mono source,
bitmap atlas, metrics and generated glyph data retain **OFL-1.1**; the crate
therefore declares `MIT AND OFL-1.1`. The source is the unmodified
[`fonts/ttf/RobotoMono-Regular.ttf`](https://github.com/googlefonts/RobotoMono/blob/895ec691990d041dd727c7b5afa3ce56525d98e6/fonts/ttf/RobotoMono-Regular.ttf)
at `googlefonts/RobotoMono` revision `895ec691990d041dd727c7b5afa3ce56525d98e6`,
SHA-256 `af0bff7599c3df3831755c16e39b3c496df74b8c8d8a1161b14dc8461be17cb4`.
Copyright 2015 The Roboto Mono Project Authors
(https://github.com/googlefonts/robotomono). The exact upstream
[`OFL.txt`](https://github.com/googlefonts/RobotoMono/blob/895ec691990d041dd727c7b5afa3ce56525d98e6/OFL.txt)
has SHA-256 `50ab8dd54680d3473f649c9db86fece88434d097c7834475c1c72d2f8c429215`
and declares no Reserved Font Names after the copyright statement.

Transformation occurs in the renderer's independently authored MIT tooling:
337 selected common-Latin/symbol scalars are rasterized offline at 20 px with
Pillow 11.1.0 / FreeType 2.13.3, independent scalar masks without shaping or
kerning, into the immutable ATX1 atlas and metrics. Renderer
`assets/manifest.json` records exact source, author, licenses, tool image,
options and output hashes. The synthetic white texture remains MIT. The editor
consumes those immutable resources through the shared renderer and performs no
font lookup, parsing, rasterization, shaping or fetching.

When the locked released graph contains this UI crate, binary packaging copies
its exact upstream notice to `licenses/OFL-RobotoMono.txt`. The font notice
helper uses offline Cargo metadata, exact manifest/lock coordinates, the
registry archive checksum and byte comparisons against its bounded, path-checked
archive members, plus the reviewed
asset manifest/source/notice hashes in `policy/font-notice.json`. Package
provenance records that registry coordinate and the exact embedded asset and
notice hashes; it refuses missing or altered staged notices. Local candidate
paths/patches are refused by production packaging. The renderer checker owns
source cmap/metrics validation; this editor helper only verifies the immutable
producer assets and attribution. The original neutral-sample asset allowlist
is independent of these dependency assets and remains exact.

The cargo-deny OFL exception uses the exact package specification
`tilewright-render-ui@0.7.1`, following the pinned
[cargo-deny 0.20.2 license exception](https://github.com/EmbarkStudios/cargo-deny/blob/0.20.2/docs/src/checks/licenses/cfg.md#the-exceptions-field-optional)
and [exact PackageSpecs syntax](https://github.com/EmbarkStudios/cargo-deny/blob/0.20.2/docs/src/checks/cfg.md#exact).
No global OFL allowance is added.

## Separate Unicode font-pack delivery

The editor packages the unchanged source fonts and original per-family OFL
notices in `font-pack/`, separately from the shared renderer's public crate.
The crate supplies only the exact `assets/unicode-font-pack.json` metadata.
Production preparation/staging obtains this metadata from the immutable
checksum-verified `tilewright-render-ui` 0.7.1 registry archive; local candidate
metadata is an explicit separate mode and never establishes registry proof.

The 17 faces are Noto Sans, Arabic, Hebrew, Devanagari, Bengali, Gujarati,
Gurmukhi, Tamil, Telugu, Kannada, Malayalam, Oriya, Sinhala, Thai, SC CJK,
KR and monochrome Emoji. They derive from the OFL directories of `google/fonts`
revision `6c1dbe27dcf7d817d4b62cec033ba77fa8e2a5ec` without modification,
subsetting, renaming or relicensing. Font bytes total 39,611,796 and original
notices total 74,478. The exact manifest SHA-256 is
`94b81a712fb41a66eb744c88ae2ed312cc5d0ec1a491e9821fcc278d99410f13`
(423,554 bytes). The manifest records every upstream and destination path,
author, copyright, license, byte count, digest, original notice and Reserved
Font Name. Noto Sans SC and KR retain Adobe's Reserved Font Name `Source`; all other
notices declare none. Font data and the inventory manifest remain OFL-1.1.

`tools/prepare-font-pack.py` performs acquisition only in explicit
`prepare --acquire` mode: fixed-revision public HTTPS paths, no redirects,
ambient proxies, cookies or authentication, exact sizes/hashes and a fresh
output. Production packaging verifies the exact file/directory set and hashes
again; it retains all 17 notices, binds provenance to the registry UI archive
and checks every font, notice and manifest component/hash/license in the binary
CycloneDX SBOM. Runtime receives only an explicit verified local root and does
not download or discover OS fonts. Shared renderer code owns font parsing,
shaping, bidi and rasterization; editor code does not duplicate those facilities.

The 49,070 mapped scalars cover the selected common Latin/Greek/Cyrillic,
Arabic/Hebrew, Indic/Sinhala, Thai, SC CJK, Korean Hangul and monochrome emoji
repertoire. Neither scalar coverage nor these packaging checks imply every
Unicode scalar or shaping/variation sequence is supported.

The delivery helper `tools/font_pack.py` and synthetic adversarial tests
`tools/test_font_pack.py` are copied verbatim from the independently authored
MIT files in `seraphinastudios/tilewright-renderer`, local reviewed candidate
commit `9072863707c64e0de29051cc565a6590232f6355`. Transformation: none; no font
parser, upstream implementation or rasterizer source is copied. Exact helper
SHA-256: `cbffc180a44fc473ecad0e7fcd9b6b50e32b4a06b7b3f404a0999a7be6ea6469`; test SHA-256: `7a92e12aebbb2ab57a41884cd0af9270df129023b721d070a451b0d1086a19e0`. Preserve their MIT headers and
this provenance when updating the shared delivery seam. This record covers
the helper copies only; the published UI crate's registry provenance is
recorded above.
