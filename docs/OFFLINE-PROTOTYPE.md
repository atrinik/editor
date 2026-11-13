# Offline editing prototype

The prototype targets World 0.2.0 and renderer 0.7.0. Release builds require
the exact published registry dependencies; local source candidates do not
establish released-consumer acceptance. The generic
editor opens ordinary World data and its strict `preview.jsonc` catalog. A
separate game-specific toolkit adapter imports a selected external map and
exports a checked subset of edits. The editor contains no game-specific
parser and never starts commands selected by project data.

This is a schematic editor demonstration, with original MIT geometric art.
It does not claim artwork, gameplay or visual parity with the external game,
which remains the active game content/runtime. Original map bytes, licensing
and import baseline belong to the external adapter workspace and are never
bundled here.

## Fresh checked session

`tools/provision-prototype.py` copies an explicitly supplied sample or imported
World into a **new absent** task directory, with distinct World, recovery and
evidence children. It refuses links and special input files, bounds the copy,
uses private 0700 directories and 0600 files, syncs creation, records directory
identities and kernel FD mount IDs/filesystem types, rejects volatile/unknown
backing (including tmpfs, ramfs and overlay), rechecks the retained identities
at launch and passes already-open directory descriptors to the child editor.
Source paths are data only. The executable is an explicit trusted operator
argument; it is never read from the project.

```sh
python3 tools/provision-prototype.py \
  --world crates/tilewright-editor-testkit/fixtures/prototype-neutral \
  --destination /absolute/private/task-session \
  --editor target/debug/tilewright-editor \
  --font-pack /absolute/prepared/font-pack
```

A Linux binary archive includes the same trusted helper and authored sample.
From the extracted archive, use Python 3 (standard library only):

```sh
python3 tools/provision-prototype.py --world samples/prototype-neutral \
  --destination /absolute/private/new-session --editor bin/tilewright-editor \
  --font-pack "$(pwd)/font-pack"
```

The helper never adopts existing state, changes permissions
on owner directories or deletes failed sessions. Inspect and preserve them;
choose a different new session directory for a new attempt.

Editing is bounded to Linux local filesystems checked by World. Recovery must
have mode 0700, current ownership, accepted ACL/label policy and directory sync.
A failed check blocks editing; there is no unchecked-access flag. The recovery
root and exclusive lock remain retained for the session. Its exact inverse
records remain after successful edits and failed or uncertain publication.
Existing records are reported and refused for a new session; automatic crash
replay and persistent history are outside this prototype.

The released storage API cannot prove arbitrary user-supplied roots do not
alias or securely create a new independently rooted child. The provisioning
helper's fresh Linux directory identities are deployment evidence for this
bounded run. Descriptor retention prevents later pathname replacement from
redirecting an already-open root. The app does not offer arbitrary recovery
path selection as an equivalent guarantee. Same-UID hostile writers remain
outside this private-directory/advisory-lock model.

## Controls and review

The native window presents the prepared scene through the shared wgpu/SDL3
adapter and remains open until the user closes it. A toolbar selects Select,
Move, Terrain or Place mode and exposes Clear, Undo, Redo and Delete. Terrain mode
paints with the drag brush described below; deletion and placement are described after it. The inspector shows
the selected object's identity, position and schema fields, or the selected
cell and explicit terrain choices. Only profile-authorized scalar fields are
editable; structured values remain read-only.

Click an editable field or position and type. Place the caret by clicking
(Shift-click extends), with the arrows (Shift extends), Home/End or
select-all; edit with Backspace/Delete or the clipboard, then choose Review. Long focused input keeps its tail and caret visible.
Fields are single-line: typed, pasted or composed text containing a control
character, U+2028 LINE SEPARATOR or U+2029 PARAGRAPH SEPARATOR is rejected as a
whole and leaves the draft unchanged. A stored value that already contains them
opens unchanged and keeps them unless the user deletes or replaces them.
Text focus consumes world shortcuts. The shared renderer shapes text using the
explicit Unicode font pack; missing-glyph diagnostics preserve the complete
original value and prevent a lossy visual replacement. Exact native input,
selection, bidirectional and composition behavior require the corresponding
interactive acceptance evidence.

The source review panel shows change-centered before/after sections and can
scroll. Unchanged omitted context is marked explicitly. Complete changed spans
must fit the 64 KiB escaped-review budget and 32-file bound; an oversized review
disables acceptance and retains the pending plan for cancellation. The complete
sealed source images remain authoritative. The terminal also prints a bounded
planned source diff and diagnostics; the sealed plan retains the exact images.
Selection uses exact reference-renderer semantic masks for the matching frame;
terrain mode uses a separate opaque cell-reference scene. SDL logical pointer
positions convert to physical pixels once. Opaque panel hit regions consume
input before world picking. The world remains full-window behind the panels;
the editor does not translate or rasterize world geometry. Resize/camera/session changes
invalidate earlier masks.

### Terrain drag brush

In Terrain mode, choose an explicit palette value, then press on a cell and
drag. One press-move-release gesture is one brush stroke. The press selects
its cell through the ordinary exact cell mask; selection then prepares an
identical frame, and the stroke keeps that frame's exact cell mask for its
samples, so each pointer sample costs no extra render.
The brush stamps its footprint at every sample and along the straight cell
line from the previous sample, so a fast drag leaves no gaps; a single jump
of more than 4,096 cells stamps only where it lands. Footprint cells outside
the catalog's editable view bounds are dropped. Samples over panels or map
background add nothing, and the next cell hit joins the line from the last
covered cell, as the earlier brush prototype did.

The brush is a square (the default) or a disc around the pointer's cell, with
a radius of 0 (one cell) to 8 (17 x 17). A disc covers the cells whose
centres lie within half a cell past the radius. The Terrain sidebar shows the
brush and offers smaller, larger and shape controls; `[` and `]` change the
size and `B` the shape. A stroke keeps the brush it began with and covers at
most 4,096 distinct cells (the default view's cell budget); a stamp that would
pass the cap is refused whole and the stroke stops growing.

While the button is held the status line counts the covered cells; nothing is
prepared or drawn as changed yet. Release turns the whole gesture into one
semantic `terrain.set` operation in one World transaction, with each changed
cell expecting its committed value; cells that already hold the chosen value
are left out, and a stroke that changes nothing prepares nothing. That one
transaction becomes the pending diff: the scene shows it and the review panel
lists it, and only Accept publishes it. History's World undo check covers the
whole stroke before the diff is shown, Accept retains one exact inverse
recovery record for it, and Undo and Redo restore or repeat the whole stroke
in one reviewed step.

A stroke can start only when no change is pending; it never merges into an
existing pending diff. Escape abandons a running stroke, and other world
shortcuts are ignored until release. A resize, rebuild, focus loss or
minimize ends the stroke without preparing anything. A new press (after a
missed release) also ends it and does nothing else. Motion that arrives
without the button held finishes the stroke where the release was missed.
The brush never saves on release: there is no autosave.

### Deleting objects

Select an object, then press Delete or choose Delete in the toolbar or the
inspector. Deletion is offered only when the catalog's optional
`editing.objects` list has an entry for the object's prototype, and only for a
top-level object in the active view (the catalog's plane and spatial layer,
with its tile inside the view bounds); otherwise the control is disabled and
the key reports why. A focused text draft keeps the Delete key for itself. One
deletion is one World transaction with one `object.remove` addressed by uid
and guarded by the object's prototype; the object's inventory goes with it.
It joins the pending fence like every other edit. The review panel shows the
diff with warnings above it: when the object holds inventory objects, when a
portal anywhere or an anchor in its plane targets its tile (they keep targeting
that tile), and always that its uid, and any uids in its inventory, are removed
and must not be reused. The warnings have their own small allowance outside
the review budget; they inform and never refuse the deletion or disable
Accept. Accept retains one inverse recovery record;
Undo reinserts the object with its exact source bytes, and when the deletion
emptied a chunk, its file is deleted and Undo restores it.

The same `editing.objects` entry also grants movement and edits of the fields
whose names its `fields` member lists, with the same active-view scope. The
entry carries no field values. The per-uid `movable_uids` and `fields` grants
keep their own reach and combine with it.

### Placing objects

Choose Place in the toolbar or press P (a focused text draft keeps P for
itself). The sidebar lists the prototypes the catalog's `editing.objects`
entries grant, one row each, sorted by prototype id, with the chosen one
marked; entering Place mode chooses the first row when none is chosen, and
clicking a row (its thumbnail included) chooses it. Without any entry, Place
is disabled and P reports why. Click a tile, empty or not: the exact cell-reference mask
picks it, as in Terrain mode. One click is one World transaction with one
`object.insert` on that tile of the active view (the catalog's plane and
spatial layer, inside the view bounds), appended on top of the tile's
stack. The new object carries a new uid, the entry's render `layer` and the
field values of the entry's optional `defaults`; World validates it against
the profile schema before the diff is shown, and refuses it, writing
nothing, when the defaults do not satisfy the schema. Nothing can be placed
while a change is pending.

A filter field sits at the top of the sidebar in Place mode, above the
scrolled rows. Click it, or press `/` (or keypad divide) in Place mode, to
focus it, then type; clicking always works, while `/` was only tested on
layouts where it needs no modifier. the palette
keeps the prototypes whose id contains the text, ignoring case, and its
header counts the matches. Case is folded one character at a time with
Unicode's full lowercase mapping, without locale tailoring or context
(capital sigma always becomes σ, never final ς) and without normalization,
so a precomposed accent does not match a decomposed one; text still being
composed in an input method does not filter until it is committed. The field
takes the single-line rules of other inputs (control characters and U+2028
and U+2029 are refused) and holds at most 64 bytes of UTF-8. While it is
focused every key is text: P, Q, Delete and the pan and tool keys type
or edit, they do not act. Enter, or a click anywhere else, releases focus
and keeps the filter; the click also does what it normally does (a tile
click in Place mode places, a row click chooses, a toolbar click acts). Esc
clears the filter and releases focus. With the field
unfocused, Esc first clears a non-empty filter, and only the next Esc
cancels a pending change as before. The filter is view state only: it is
bound to no selection or World revision, never creates a transaction, and
never changes what may be placed. Under the filter field a pinned row, which
never scrolls and is not clickable, always names the chosen prototype with
its thumbnail: `Placing: crate`, or `Placing (hidden): crate` when the filter
hides its row. A hidden choice stays chosen and a tile click still places
it; its palette row is marked only while shown. When nothing matches, one neutral
"No prototype matches the filter." row stands in for the list. The filter
stays editable while a change is pending, when the rows are disabled as
before. Re-entering Place keeps the filter; switching to another tool clears
it, so Place opens with the whole palette.

Each palette row is 40 logical pixels tall, and starts with a thumbnail of
its prototype: the prototype's ready preview texture from the catalog, the
same resource the World view draws, fitted into a 36-pixel square with its
aspect ratio kept, centred and snapped to whole physical pixels. Thumbnails
are extra textured quads in the composed workspace scene and need no
renderer change. Only rows laid out in the sidebar get one, and rows are
laid out only when they fit whole, so a thumbnail never reaches the toolbar,
the review panel or the World view; scrolling never cuts one in half. They
are not pickable and are not in the World scene, so the exact semantic masks
used for selection, painting and placement are unchanged by them; a press
on a thumbnail is a press on its row. Disabled rows show a grey, faded
thumbnail.

The uid is `o.` followed by 128 random bits from the operating system in
lowercase base32 (26 characters), as the World format recommends, so uids
placed on different branches do not collide. One that already names an
object in the World is drawn again, a bounded number of times. The scripted
demonstration uses a fixed, seeded sequence instead, so its evidence is
reproducible.

The review panel names the prototype, the tile and the new uid above the
diff, and warns, without refusing, when an object of the same prototype
already stands on the tile. Accept retains one inverse recovery record and
selects the new object. It is then an ordinary object of its prototype:
the same entry lets the author select, move, relabel and delete it. Undo
removes it with byte-exact source, and a placement that created a chunk
file is undone by deleting that file.

Planned edits are visible before acceptance. Accept retains the exact inverse
before World publication; Cancel publishes nothing. Undo and redo prepare World
transactions and require acceptance through the same path. A pending plan must
be accepted or cancelled before another edit, undo or redo is prepared. Valid
semantic selections survive acceptance. Native input cannot accept a pending
change before its current frame has been presented. An uncertain
applied/durable/cleaned result blocks further editing and preserves its record.
Reopen is explicit inspection/reload, never blind recovery or retry.

The first external-map edit moves an eligible UID-bearing top-level object only
when its original block has one existing `x` and `y`, within original bounds.
Same-chunk coordinate changes use guarded generic World path transactions to
preserve source object order. Cross-chunk relocation still uses World's
`object.move` and may exceed the adapter's export allowlist; the exporter refuses
unsupported structural/order changes. Other fields, ordering and bytes are
preserved by the adapter's checked export.
The neutral sample also exposes a typed field and terrain paint operation.
External-map floors remain objects; no generic terrain capability is inferred.

Export is a separate explicit toolkit CLI operation on committed World data.
Pending scenes are never export input. Keep the original content tree read-only
and export into a new task-owned output. See the adapter's own usage and tests
for its manifest/stale-source/unsupported-edit rejection contract.

## Evidence and limits

`--demo-script` drives the same session actions, command history and scene
preparation as interactive input, including a placement with seeded uids and
a palette search (it types `A`, captures `palette`, then places the first
match). It is a deterministic scripted demonstration,
not a claim of human input. `--reference` produces composed workspace CPU
reference evidence and
must be reported separately from an actual visible native window run. Native
presentation/backend/adapter evidence cannot be inferred from passing unit tests
or CPU frames. Screenshot/video artifacts must identify which path produced
them and the exact source revision and dependencies.

Packages include the exact allowlisted OFL Unicode font pack and authored MIT sample data/resources, immutable
registry references, notices, checksums, SBOM and provenance. Imported worlds,
real game maps/art, adapter baseline, recovery records and session evidence
are excluded. Windows build/test/package success is not Windows durable-editing
or native D3D12 qualification.

Later work includes a full widget/text-editing system, arbitrary-root
provisioning APIs, automatic crash replay, persistent undo, additional
external-map edit forms, original art/effects qualification, broader platform
durability and game-runtime cutover. Each needs its own evidence and
authority; none is implied by this prototype.

## Explicit Unicode font preparation

The native editor requires `--font-pack ROOT`. Binary packages carry the fixed
`font-pack/` directory beside `bin/`; the provisioner forwards that explicit
absolute root. It checks the exact immutable manifest and all font/notice
bytes before creating a session. It does not search operating-system fonts or
fetch anything. Source candidate provisioning records local byte verification
and `registry_proof: false`, independently of released-consumer evidence.

Prepare fonts as a separate explicit build step after `cargo fetch --locked`:

```sh
python3 tools/prepare-font-pack.py prepare --root . --acquire \
  --output /absolute/new-prepared-font-pack
tools/package-linux.sh /absolute/new-dist 0.1.0-test.1 \
  /absolute/new-prepared-font-pack
TILEWRIGHT_PREPARED_FONT_PACK=/absolute/new-prepared-font-pack tools/validate.sh
```

Preparation obtains the manifest only from the exact checksum-verified registry
`tilewright-render-ui` crate. Public upstream downloads use the manifest's
fixed revision, paths, sizes and hashes, with redirects, proxy inheritance,
cookies and authentication disabled. Packaging verifies the prepared pack
again, copies every original OFL notice, records the binding to the UI archive,
and checks font data identities/hashes/licenses in the binary CycloneDX SBOM.
A missing pack or notice, extra file, altered payload, link, changed manifest
or local dependency override refuses production packaging. Preparation refuses
existing destinations; inspect and remove failed partial preparations explicitly.

For a local source candidate, stage a separate review artifact explicitly:

```sh
python3 tools/prepare-font-pack.py candidate --root . \
  --candidate-ui /absolute/renderer/crates/tilewright-render-ui \
  --font-pack /absolute/renderer/assets/unicode --stage /absolute/candidate-stage
```

This mode labels its provenance as a local candidate and cannot pass the
production registry verifier. The public UI crate carries only the exact
423,554-byte metadata manifest; the approximately 39.6 MB unmodified font
payloads are distributed separately and loaded from the explicit pack root.
The pack covers a finite repertoire, including common Latin/Greek/Cyrillic,
Arabic/Hebrew, the selected Indic scripts, Thai, SC CJK, Korean Hangul and
monochrome emoji. Missing scalars and shaping/variation sequences still require
diagnostics; this is not a promise of universal font coverage.
