# Offline editing prototype

The prototype targets World 0.2.0 and renderer 0.7.1. Release builds require
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
Move, Terrain, Place or Zone mode and exposes Clear, Undo, Redo and Delete. Terrain mode
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
The inspector row and the field draft show each such stored character as one
marker on the same line: `⏎` for a line break (LF, CR, VT, FF, NEL, U+2028
and a CR LF pair, which editing treats as one character), `¶` for U+2029,
`→` for a tab and `⬚` for any other control character. A row below the field
counts them ("Contains 2 line breaks"). The marker is display only: the draft,
the clipboard, the comparison with the stored value, the reviewed `SetField`
value and the source review keep the real characters. The caret moves over a
marker as over one character, and Backspace or Delete beside it removes
exactly that stored character (both characters of a CR LF pair). A draft that
still contains one can be reviewed as it is. When a prepended or combining
character next to the stored break would otherwise join the marker, a
zero-width space is drawn between them; it is part of the marker and never
stored. A stored character that is itself one of the marker symbols (`⏎`, `¶`,
`→`, `⬚`) is ordinary text and looks the same as a marker. The count row
describes the stored value and counts only real stored breaks and controls; it
does not change while a draft is edited. Invisible format characters, such as
bidirectional controls and joiners, are not control characters and are not
marked.
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

While the button is held, a live preview marks the cells the stroke would
change if released now: a neutral translucent white tint on each such cell,
and the status line counts the covered cells and how many of them would
change. The marked cells are exactly the cells the release puts into its
`terrain.set`: the editor session computes both with one function from the
same stroke, so covered cells that already hold the chosen value stay
unmarked, cells outside the view are never covered, and a stroke that would
change nothing marks nothing. The tint is drawn on copies of the prepared
exact cell-reference quads, the quads the cell mask itself is rasterised
from, so it has each cell's projected shape, including isometric diamonds
and tiles that terrain heights bend or stretch. The copies are placed in the
World's own draw order. The first time a prepared scene needs it (when the
zone overlay or a stroke preview is shown), the preview compiles the World's
terrain, then the cell reference tiles, then the World's placed objects into
one scene with the shared renderer, purely to learn where each cell falls
among the World's draws, and keeps that for the prepared scene; sessions
that never show an overlay never pay for it. A copy then draws directly
after the last World draw before its cell: after the cell's own terrain, or,
for a cell without terrain, at that cell's own position in the order (not at
an earlier cell's), in isometric and top-down views alike. Because the cells
come before the placed objects, an object on the grid's own ground render
layer, which shares its tile's order key, is still drawn after the mark. A
cell before every World draw draws first. If that combined ordering scene
cannot be compiled (a defensive case that the default limits do not reach),
no mark is drawn and
the status line adds "Preview unavailable for this view."; the release still
prepares the whole stroke. Anything the World draws later covers the mark as
it covers the terrain: an object standing on the
cell, a nearer raised tile or a higher ground layer hides the mark there.
Where a terrain art's outline differs from the reference cell shape, the
mark can reach a thin edge of a farther tile that the click mask gives to the
changed cell, and two neighbouring marks can overlap along it. For the same
reason a nearer cell's mark or tint can cover part of a farther object that
the nearer tile's art does not actually cover; ground art that fills the
tile's reference shape does not show this. The white tint
has little contrast on very light terrain; this is not addressed yet. The
copies are unpickable: they never reach a semantic, depth or coverage plane,
so what a click picks is unchanged. Each pointer sample
reuses the cached prepared World scene; only the overlay and the interface
are composed again, and no World scene is prepared. The overlay is bounded by
the stroke's 4,096-cell cap and is a subset of a scene that already met the
renderer's scene limits; should the World, the overlay and the interface
together still exceed them, the frame is drawn without the overlay and the
status line says the cells are too many to preview, while the release still
reviews every cell. Nothing is prepared, written or added to History until
release, and Escape (or anything else that abandons the stroke) removes the
preview. Release turns the whole gesture into one
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

### Zones

Zone mode (the toolbar's Zone, or N) shows which zone every tile of the view
is in. World keeps a zone per tile and layer in each chunk's `zone` grid; an
empty cell, or a chunk without a zone grid, is in the plane's default zone,
and a tile with neither has no zone. A tile's display name is the first zone
walking up its parent chain that has a `name` (an unnamed child shows its
parent's name). The editor computes this walk; it stops at an unknown parent,
a repeated zone or 64 zones, although World already refuses cycles.

The overlay tints each tile of the view that has a zone; tiles without a zone,
and tiles whose chunk does not exist, get no tint. Tints are copies of the
prepared exact cell-reference quads (the quads the cell mask is rasterised
from), unpickable and placed in the World scene's draw order exactly like the
terrain stroke preview's marks, so each draws where its tile's ground is
drawn, before the objects on it, any nearer raised tile and higher ground
layers, which cover it as they cover the ground: the part of a tile an
object hides shows no tint. A tile without ground keeps its own place in
that order, so a zoned but unpainted area stays visible and is covered only
by what is on it or nearer, exactly as ground there would be. Objects on the
grid's own ground render layer (a placed object may use any granted render
layer, on sub-layer 0 or above) are drawn after their tile's tint too. A nearer tile's tint does draw
over the part of a farther object that overhangs that nearer tile, as the
nearer tile's ground would, including where the tile has no ground. When the
cells' places in the draw order cannot be computed for a view (see the
terrain brush), no tint is drawn and the status line says "Zone overlay and
stroke preview unavailable for this view." The semantic, depth and coverage planes, and so every pick,
are unchanged.

Colours come from eight fixed tints (the Okabe-Ito colour-blind-safe set with
black replaced by indigo) at 40 % opacity. They are assigned over all zone
definitions of the plane in the shown World (the pending one while a change
is reviewed), sorted by id: each zone prefers the tint a stable hash of its
id selects, and a zone whose preferred tint an earlier zone took gets the
next free one. A zone's colour therefore changes only when the plane's zone
definitions change, never because of a stroke or what is in view. Creating
or deleting a zone can move other zones' tints (a zone sorted after it whose
preferred tint collides); the overlay, the legend and its swatches follow the
pending definitions while the change is reviewed, and the committed ones
again after Accept, Cancel or Undo. Two zones may also get similar tints
(the set has two oranges); the legend's ids and the inspector stay the
reliable cue. Zones share a tint only when the
plane defines more than eight, and the legend then says "shared tint" on
those rows. The legend's swatches are the tints drawn opaque; a translucent
tint over varied terrain does not match its swatch exactly, so the tile's
shape in the overlay and the inspector's zone line are the reliable cue.

If the overlay would take the frame past the scene limits it is left out and
the status line says so ("Zone overlay left out: too many cells for one
frame."). During a stroke the zone tints and the stroke preview are composed
together, so both are dropped together and the status line says "Too many
cells to preview."

The overlay is always on in Zone mode. In the other modes it is off by
default and O (or the sidebar's "Show zones and badges  O" row; "Show
zones  O" in Anchor mode) toggles it; the choice is kept across modes. The
anchor and portal badges (see Anchors) show wherever the zone overlay shows,
and always in Anchor mode. The World pixels of the existing demonstration captures of
other modes are therefore unchanged; their interface did change: the toolbar
buttons are narrower to make room for Zone, the sidebar has the toggle row,
and the inspector of a selected tile has zone lines.

In Zone mode the sidebar lists every zone of the plane, sorted by id, with its
tint as a swatch, its display name and how many tiles of the view it holds,
or "not in view". A last row stands for the plane's default zone. Only the
first 256 zones are listed, and then the header still counts every zone and
a row says "First 256 zones shown"; there is no filter yet. Zone ids come first in
these rows; when a row is too narrow the display name and count are dropped
first, and then the id is elided in its middle, keeping its head and its
tail, because hierarchical ids share their beginnings. The inspector shows a
selected tile's zone (marked when it is the plane default), its parent chain
outermost first (outer zones give way to `...` first; the tile's own zone is
kept whole) and its display name, in every mode that selects tiles.

Painting zones needs the catalog's optional `editing.zones` member,
`{"paint": true, "manage": false}`: capability flags, never a list of zone
ids, so the catalog keeps loading when zones change. `manage` grants zone
management (below). Without `paint` or `manage` the zone rows are disabled, a press
only selects the tile and the status line says painting is not granted.
With it, choose a zone row (or the default-zone row) and drag: the gesture,
brush size and shape keys, caps, view clipping and pending fence are those of
the terrain brush. Samples come from the ground cell mask; the release writes
the `zone` grid. Tiles whose chunk does not exist are left out of a zone
stroke like tiles outside the view, because painting them would create a
chunk document holding only a zone grid; tiles of an existing chunk that
merely have no ground stay paintable, and a stroke that reaches no existing
chunk is refused, saying its tiles are in map chunks that do not exist.

One stroke is one World transaction with one `terrain.set` on the grid
`zone`, a cell entry with `expect` for every covered cell whose own value
changes, and value `null` for the default-zone row (the cell empties and
shows the plane's default zone again). Cells are compared by their own
value, not the zone they display, so painting the default zone onto an empty
cell writes it explicitly and is a change. World adds or removes the chunk's
zone palette entries and the zone grid itself as needed; Undo and Redo
restore the exact bytes. The review shows a neutral note per kind of change,
such as "Zones: 5 tiles move to path (Stone path).", "Zones: 1 tile returns
to the default zone, meadow (Quiet meadow)." or, for empty cells given the
default zone explicitly, "Zones: 2 tiles now hold meadow (Quiet meadow)
explicitly (they showed it as the plane default)."

While a zone stroke runs, its live preview shows the result: the cells the
release would change are drawn in the target zone's tint at 80 % opacity,
twice the resting 40 %, in place of their resting tint (the default-zone row
shows the plane default's tint, or no tint when the plane has no default).
The terrain brush keeps its white mark, which draws over a zone tint of the
same cell when the overlay is toggled on.

#### Managing zones

With `editing.zones.manage` a zone row of the Zone mode list also chooses
the zone to manage (without `paint` it only does that, and the status line
says so), and the sidebar shows it below the list: its id, marked as fixed;
its display name, or "none" and the name it inherits; its parent; how many
tiles of the view show it, and how many of those only as the plane default
(an empty cell shows the default zone without holding it); how many tiles
hold it explicitly in the whole plane ("Held by 3 tiles in the plane"), every
chunk of every layer, not only the view; chunks whose zone palette lists it with no tile; its child zones;
whether it is the plane's default zone; and its `properties`, read-only, as
compact JSON. A zone draft and the parent list belong to the zone they were
opened for: choosing another zone, leaving Zone mode, or the zone ceasing to
exist in the shown World (deleted, or its creation undone) ends them, and the
choice itself is dropped when the zone no longer exists. The draft's hint
names the zone it edits ("Name of meadow; empty = none") or the parent a new
zone goes under. Each is an
edit of the plane's `zones.jsonc` reviewed as one World transaction, with
one recovery record when accepted and byte-exact Undo and Redo:

- **Zone ids are fixed.** Nothing changes an id; "Edit display name" opens
  the text draft with the current name (the single-line rules of field
  drafts apply, and stored line breaks show as markers). Review writes one
  `set` of `["zones", {"id": X}, "name"]` expecting the old name, or an
  absent `test` and a `set` when the zone had none. An empty draft means no
  name: the member is removed (expecting the old name) and the zone shows
  the name its parents give it, if any. A name has at most 256 characters,
  as the format says.
- **Parent.** "Parent: ... (change)" lists the plane's zones except the zone
  itself and its descendants, so a cycle cannot be chosen, plus "No parent
  (top level)". Like the zone list it shows the first 256 in id order and
  then says how many there are; it is computed when it opens and again when
  zone definitions change. A choice writes one `set` or `remove` of the `parent` member,
  guarded the same way. The Session refuses a parent that is unknown, the
  zone itself or one of its descendants on its own, whatever list was
  shown; World's validation stands behind it.
- **New zone.** "New zone under X" or "New top-level zone" opens a draft:
  type the id, then optionally a space and the display name (ids cannot hold
  spaces). The name part is trimmed, and only whitespace means no name; a
  display-name draft keeps its text exactly, as field drafts do. The id must be a World identifier (`^[a-z0-9][a-z0-9_.-]{0,127}$`)
  the plane does not use; the draft stays open until it is. Review writes an
  absent `test` of `["zones", {"id": new}]` and an `insert` at the end of the
  `zones` array, or `document.create` of `zones.jsonc` when the plane has
  none (Undo deletes that document again). A new zone holds no tile; it is
  in the list at once and the brush paints it.
- **Delete.** Only an unused zone can be deleted: no tile of any chunk on
  any layer holds it and no chunk's zone palette lists it, no zone has it as
  parent, and it is not the plane's `default_zone`. Otherwise the row is
  disabled with those facts shown above it and one "In use: ..." line per
  reason under it, and the Session refuses with each reason and its count
  ("zone is in use and cannot be deleted: 6 tiles hold it, in 3 chunks on 2
  layers; 1 child zone (meadow.pond); it is the plane's default zone"). A
  chunk palette that lists the zone with no tile (only a hand edit leaves
  one) is its own reason, naming the first three such chunks by layer and
  chunk ("layer 0 chunk 0_0", the chunk file `layers/0/0_0.jsonc`); to drop
  the entry, paint one tile of that chunk with the zone and then back: a
  stroke drops a zone's entry when its last tile in the chunk is emptied or
  repainted, while strokes of other zones keep it. Deleting writes one `remove` of the entry,
  expecting its exact canonical JSON. Deleting the last zone leaves an empty
  `zones` array; the document stays.

The review lists one neutral note per definition change, worked out by
comparing the zone definitions before and after, so Undo and Redo are
described too: "Renames zone meadow from 'Quiet meadow' to 'Open meadow'.",
"Names zone X 'N'.", "Removes the display name 'N' of zone X; it shows 'M'
from zone Y." (the zone up its chain the shown name now comes from),
"Moves zone X under Y.", "Makes zone X a top-level zone.", "Creates zone X
under Y, named 'N'." and "Deletes unused zone X.". Editing `properties` or
`custom`, setting the plane's default zone, and merging or splitting zones
are not offered; a profile whose `zone_properties` schema requires a
property makes World refuse a new zone, and the refusal is shown with
nothing pending.

### Anchors

An anchor is a named tile, `{id, layer, at}` in the plane's
`planes/<p>/anchors.jsonc`; a portal on an object can lead to one. Anchor
mode (the toolbar's Anchor, or K) shows them:

- **Badges.** Every tile of the view that holds an anchor gets a small light
  badge in its first corner (top left in the top-down sample), and every tile
  that holds an object with a portal, its own or one carried in its
  inventory, a small dark badge in its last corner (top right). Each badge has
  a 1 px outline of the other tone, so the light one reads on light ground and
  the dark one on dark ground. A badge is the tile's own exact cell shape
  scaled down toward that corner. Badges are markers, not tints: they are
  drawn over objects, above all World content (after the last World quad,
  below the interface), so an object on the tile, even one that covers the
  whole tile such as a door, never hides one. They are unpickable: they never
  reach the semantic, depth or coverage planes, and
  a click on a badged tile still picks that tile. The badges show always in
  Anchor mode and, in the other modes, whenever the zone overlay shows (Zone
  mode, or after O). In Anchor mode O adds the zone tints. Anchors on other
  layers or outside the view are listed, not drawn. Badges need none of the
  World draw-order slots the zone tints use, so they still show when the zone
  overlay cannot be drawn.
- **List.** The sidebar names what the badges mean (each legend row carries
  its badge's colour) and lists every anchor of the plane, sorted by id: its
  layer and position, "in view" or "not in view", and how many portals in the
  whole world name it. Only the first 256 are listed, with the real total in
  the header. Ids come first and are elided in the middle when a row is too
  narrow, as zone ids are. A row chooses the anchor; the chosen one shows its
  fixed id, its place and the objects whose portals name it (the first three:
  uid or "(no uid)", plane, layer and tile, "carried" for an inventory entry).
- **Inspector.** A click in Anchor mode selects the tile under the pointer
  through the exact cell mask, objects standing on it included. In every mode
  a selected tile lists its anchors and how many portals name each, and a
  selected object with a portal shows its target, read-only: a plane, layer
  and tile, or an anchor id and where that anchor is. A portal without a
  `plane` member targets its own object's plane.

Editing needs the catalog's optional `editing.anchors` member, `{"edit":
true}`: a capability flag, never a list of anchor ids, so the catalog keeps
loading when anchors change. Without it the editing rows are absent and the
status line says anchor editing is not granted. Every edit is limited to the
active view's plane (create and move to tiles of the view; delete works on any
anchor of the plane, in view or not, as zone deletion does), needs no pending change, and is one World transaction on
`anchors.jsonc` with one recovery record when accepted and byte-exact Undo
and Redo; the Session checks the grant, the plane, the view and the revision
again whatever the window offered, and World's validation stands behind it.

- **New anchor.** Select a tile of the view, press "New anchor at (x, y)",
  type the id and press Enter. The id must be a World identifier the plane does
  not use; the draft stays open until it is. The anchor is created on the
  selected tile and the view's spatial layer: an absent `test` of
  `["anchors", {"id": new}]` and an `insert` at the end of the `anchors`
  array, or `document.create` of `anchors.jsonc` when the plane has none
  (Undo deletes that document again). Two anchors may share a tile.
- **Move.** Choose an anchor that is on a tile of the view, select its new
  tile and press "Move here: (x, y)": one `set` of its `at`, expecting the old
  position (its `layer` stays). Every portal that names it now leads to the
  new tile; the review lists them.
- **Delete.** Only an anchor no portal names can be deleted. Portals are
  looked for everywhere: every plane, layer and chunk, placed objects and
  inventory entries at any depth, with or without a uid. Otherwise the row
  reads "Delete: 1 portal names it", disabled, under the list of those
  objects, and the Session refuses with the count and the first three
  ("anchor is named by portals and cannot be deleted: 1 portal: object
  gate_east on main layer 0 (7, 0)"). Deleting writes one `remove` of the
  entry, expecting its exact canonical JSON; deleting the last anchor leaves
  an empty `anchors` array.
- **Ids are fixed in this version.** Nothing renames an anchor. To get the
  same effect, create a new anchor and point the portals at it, once portal
  editing exists (the next change), then delete the old one.

The review notes compare the anchors before and after, so Undo and Redo are
described too: "Creates anchor camp on layer 0 at (2, 3).", "Moves anchor
arrival from layer 0 (7, 3) to layer 0 (6, 2)." with "Portals naming it now
lead to the new tile (1 portal: object gate_east on main layer 0 (7, 0))." or
"No portal in the world names it.", and "Deletes anchor camp (layer 0, (3,
4)); no portal in the world names it.". A move or a delete also warns,
without blocking Accept, that scripts or saves outside the world may refer to
the anchor id. Editing portals is not part of this version.

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
layouts where it needs no modifier. The palette keeps the prototypes whose
id contains the text, ignoring case, and its header counts the matches. Case is folded one character at a time with
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
never scrolls and is not clickable, always names the chosen prototype in
full with its thumbnail, `Placing: crate`, so the choice stays visible when
the filter hides its palette row; it is dimmed while a pending change blocks
placing. A hidden choice stays chosen and a tile click still places it; its
palette row is marked only while shown. When nothing matches, one neutral
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
match) and, after the terrain step, a 3x3 sand drag stroke over grass and
sand captured mid-gesture as `stroke-preview` and then cancelled with Escape,
so it changes nothing. When the catalog grants zone painting it ends in Zone
mode: `zones` shows the overlay and legend, then a one-cell `path` stroke down
column 5 is captured mid-gesture with its live preview
(`zone-stroke-preview`), released (`zone-stroke`, pending review) and
accepted (`zone-painted`). When it also grants zone management, it then
renames `meadow` to "Open meadow" (`zone-rename`, pending), creates
`meadow.grove` named "Grove" under it (`zone-create`, pending), paints the
tile (2,4) with it (`zone-created`), shows that it cannot be deleted while
that tile holds it (`zone-delete-refused`: the Delete row disabled with its
reason, as the window shows it; a press on a disabled row does nothing),
undoes the paint and deletes it
(`zone-delete`, pending; `zone-deleted`). When the catalog grants anchor
editing it then enters Anchor mode (`anchors`: the badges and the list),
selects the tile (2,3) and creates the anchor `camp` there (`anchor-create`,
pending; `anchor-created`), selects (3,4) and moves `camp` there
(`anchor-move`, pending), shows that `arrival`, which the portal of
`gate_east` names, cannot be deleted (`anchor-delete-refused`: the Delete row
disabled under the object that names it), and deletes `camp`
(`anchor-delete`, pending; `anchor-deleted`). It is a deterministic scripted demonstration,
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
