# Authored geometric meadow

This is a tiny original MIT top-down editor sample by Seraphina Studios. All
pixels are newly authored integer geometry; no game art, game source, or
third-party images were copied. It illustrates editing and transitions, not
any game's visual fidelity or gameplay. COPYING contains the MIT license.

Open `fixture_world("prototype-neutral")` or this directory as an ordinary
Tilewright World 0.2 project. `preview.jsonc` declares editor-preview/1 resources,
capabilities and the default `main` plane, spatial layer 0, bounds `[0,0,8,6]`.
The marker `marker_1` starts at `(3,2)` on render layer `item`; its typed `label`
is `Trail marker`, and its nested typed `settings` holds `active` and `visits`.
Move it to `(4,2)`, change its label, or paint `floor` at the empty cell `(6,4)`
with `grass` or `sand`. A second marker, `marker_2` at `(1,4)` labelled
`Spare marker`, is the one the scripted demonstration deletes. The floor's grass/sand seam derives directional edge
prototypes through the released World's transition resolver.

The plane has three zones in `planes/main/zones.jsonc`: `meadow` (named
`Quiet meadow`), `meadow.pond` (a child of `meadow` without a name, so it shows
`Quiet meadow`) and `path` (named `Stone path`). `meadow` is the plane's default
zone. The visible chunk's `zone` grid puts `meadow.pond` on `(1,1)`-`(2,2)` and
`path` on the row `y = 5`; every other tile, and the whole chunk `[-1,0]`, which
has no zone grid, is in `meadow`. The catalog grants `editing.zones`
`{paint: true, manage: true}`; the scripted demonstration paints `path` down
column 5, then renames `meadow` to `Open meadow`, creates `meadow.grove`
(`Grove`) under it, paints `(2,4)` with it, shows its Delete row disabled
while that tile holds it, undoes the paint and deletes it.

The plane has two anchors in `planes/main/anchors.jsonc`: `arrival` on layer 0
at `(7,3)`, in view, and `lookout` on layer 0 at `(-2,0)`, left of the view.
Two `gate` objects carry the sample's portals: `gate_east` at `(7,0)` leads to
the anchor `arrival`, and `gate_west` at `(0,3)` to the location `(-2,2)` on
layer 0 of the same plane. The gate is a 16x32 standing prototype (two slate
posts under a lintel, open between them). Its `editing.objects` entry names no
field and grants `portal: true`, so a gate's portal can be set, changed or
cleared; like every entry it also lets gates be moved, deleted and placed. The
scripted move and deletion leave objects whose portal is editable alone, so
they still take the markers. The scripted demonstration then retargets
`gate_east` to `lookout`, picks the tile `(0,3)` under `gate_west` (reviewed
with a warning, then cancelled) and then `(4,3)` as its target, and clears
that portal. The catalog
grants `editing.anchors` `{edit: true}`; the scripted demonstration creates
the anchor `camp` on `(2,3)`, moves it to `(3,4)`, shows that `arrival` cannot
be deleted while `gate_east` names it, and deletes `camp`.

For negative/cross-chunk picking, use the same world with bounds `[-2,0,4,3]`.
Chunk `[-1,0]` contains ground at x=-2/-1 and the marker `marker_negative` at
`(-1,1)`; chunk `[0,0]` continues the ground at x=0/1.
The catalog grants movement and the `label` field to `marker_1` by uid, and
grants the `marker`, `stone`, `bush`, `crate` and `lamp` prototypes through
`editing.objects` (render layer `item`, field name `label`): every such object
inside the view bounds can be deleted, moved and relabelled, and all five (and
the `gate`) can
be placed, so the Place palette offers a real choice and its search has
something to narrow. The `stone` entry has `defaults`: a placed stone
gets the label `New stone` and `settings` `{active: false, visits: 0}`; a placed
marker gets no fields. The scripted demonstration places a stone on the empty
tile `(6,1)`, then filters the palette with `A` (crate, gate, lamp and
marker match) and places a crate on the empty tile `(2,1)`. Prototype grants reach only the active view, so
`marker_negative` stays read-only with the default bounds.

From the repository root:

```sh
python3 crates/tilewright-editor-testkit/fixtures/prototype-neutral/generate_assets.py
cargo test -p tilewright-editor-testkit
```

The generator's default mode checks exact bytes, hashes, dimensions, catalog
metadata and repository asset provenance without writing. Use `--write` only
when deliberately regenerating this fixture. ATX1 files are magic `ATX1`, two
little-endian u32 dimensions and exactly width*height*4 row-major RGBA8 bytes.
Ground is 32x32; the transparent amber sign is 24x40 with a distinct standing
silhouette, the grey stone is a 24x24 standing ellipse with a lighter top, the green bush
is 28x24 (three overlapping discs), the brown crate is a 24x24 framed box with
a diagonal brace, the lamp is a 16x40 dark post with a pale yellow light, and
the gate is a 16x32 pair of slate posts under a lighter lintel with a pale step. Rendering and projection are supplied by the shared renderer.
