# Read-only commands

The `tilewright-editor` binary answers four read-only questions about a world
without opening a window or an editing session: `overview`, `tile`,
`catalog` and `lint` (issue #57). The MCP server's observe tools will return
the same reports later. They are tested on Linux and Windows in CI; macOS is
expected to work, since nothing in them is platform-specific, but no CI job
builds or tests it. Editing stays Linux-only.

```sh
tilewright-editor overview [--json] ROOT
tilewright-editor tile [--json] ROOT PLANE LAYER X Y
tilewright-editor catalog [--json] [--match TEXT] ROOT
tilewright-editor lint [--json] ROOT
```

`ROOT` is a world source root, the directory holding `world.jsonc`, given as
a path the way `open ROOT` takes it (it need not be UTF-8). This and every
later command spell it the same way: a positional `ROOT` right after the
command. `--json` and `--match TEXT` may come anywhere after the command, each
at most once; an argument starting with `--` is always an option, so a root
cannot start with `--` and `--match` refuses a `TEXT` that starts with `--`
(a usage error, so `catalog --match --json ROOT` is not a search for
`--json`). `LAYER` is
a spatial layer (32-bit) and `X Y` are plane tile coordinates (64-bit).

## Read-only by construction

`overview`, `tile` and `catalog` open the world with `WorldDocument::open`
(World's default load bounds), so they can describe a world the editing
entry point refuses as over its load budget. `lint` first opens it with the
editing entry point's budget (`WorldDocument::open_prototype`). If that
succeeds, its World is the one the default load gives: both loads read the
same files with the same parser and validation, and limits can only refuse
a world, never change one (a test compares the two Worlds). If it fails and
the default load succeeds, the budget alone refused the world: lint reports
`world.over_editor_budget` and its other findings come from the default
load. If both fail, the default load's diagnostics are the load failure, as
for every other command. Each command then opens the editor catalog,
`preview.jsonc`, with the preview loader.
Nothing else is opened: no session, recovery store, provisioned copy,
transaction or publication. The crate that builds the reports,
`tilewright-editor-observe`, never constructs a `Session` and has no direct
dependency on the transaction, history or recovery crates (it reaches them
only through the ui crate, for the functions the desktop editor draws from).
`tools/check-architecture.sh` pins its direct dependencies and fails if its
non-test sources name `Session`, `History`, a recovery store, the
transaction or recovery crates, or provisioning. A test runs every command in
both forms on a copy of the neutral sample and checks that every file, its
bytes, its permissions and its modification time are unchanged and that no
entry appears beside the world.

## World text is data

Ids, names, titles, versions, field values and loader messages come from
the world. In both forms they are values, never member names or structure:

- `--json` prints one JSON document on stdout. Every escaped character (the
  set below) is written as a `\u` escape, as a UTF-16 surrogate pair above
  U+FFFF, so the document is safe to print as it is and parses with any
  strict JSON parser. Integers are JSON integers.
- Without `--json`, the same document prints as indented `name: value` lines.
  Each value is one escaped field, every escaped character written as its
  `\u{...}` escape: a world value cannot add or forge a line, move the
  cursor, retitle the terminal, reorder text or hide characters.

The escaped set (`node::escaped` in the observe crate) is every control
character (C0, DEL and C1); U+2028 and U+2029; the bidirectional formatting
characters U+061C, U+200E, U+200F, U+202A to U+202E and U+2066 to U+2069;
and the invisible or format characters U+00AD, U+180E, U+200B to U+200F,
U+2060 to U+2064, U+FEFF, U+FFF9 to U+FFFB and the tag characters U+E0000 to
U+E007F. Variation selectors are not escaped.

The text form is for people. It is not unambiguous, so scripts and agents
parse `--json`:

- a null value and the string `none` both print `none` (and `(none)`, an
  empty list or object, is also what a string `(none)` prints);
- a backslash is printed as itself, so world text holding the characters
  `\u{1b}` prints exactly like an escaped ESC;
- an empty string prints `""`, as does the two-character string `""`.

Readers that pass world text on, to a person or to a model, should label it
as data: a field value that reads like an instruction is still only a value.

## Output shapes

Every document starts with `format` and, except for refusals and load
failures, `revision`: the World revision (SHA-256 of the files the load read)
it describes. Shapes are stable within a format version; a later version adds
members or changes the tag.

The `catalog` member reports the editor catalog: `status` is `absent` (no
`preview.jsonc`), `invalid` (with the loader's `problem`) or `loaded`.

- `tilewright.editor-overview/1`: `profile` (id, version, projection, render
  and ground layers, terrain layer, terrains, layer height), `definitions`,
  `plane_count`, `planes` (each with title, kind, chunk size, default zone,
  counts of zones, anchors, stairwells, chunks, placed objects, carried
  inventory objects and portals, and per spatial layer its chunk count and
  the tile bounds `[x, y, width, height]` its chunks cover, or null when those
  do not fit in 64 bits), and `catalog`:
  the view (plane, layer, bounds), presentation, how many view tiles have a
  zone and how many each zone holds, prototype and resource counts, and
  `grants`: per-uid moves and fields, terrain grids and values,
  per-prototype object entries (layer, fields, default field names,
  portal), and the zone and anchor flags.
- `tilewright.editor-tile/1`: `tile`, `chunk` (path and chunk coordinates, or
  null), `ground` per grid, `height`, `zone` (id, whether it is the plane
  default, path innermost first, display name), `catalog.in_view`, `objects`
  on the tile in source order (uid, prototype, render layer, sub-layer, z,
  field overrides with their kind, portal target, direct inventory entries,
  carried objects at any depth, whether it has `custom` data, and `allowed`:
  what the catalog lets the author do in the active view: move, delete,
  portal and the editable field names), `anchors` on the tile with how many
  portals name each, and `arrivals`: objects anywhere whose portal leads to
  the tile, by location or through an anchor on it.
- `tilewright.editor-catalog/1`: `match`, `prototypes` (resource, anchor,
  whether placeable, and for placeable ones their layer, fields, default
  field names and portal flag, plus the terrain grids that offer them),
  `terrain_palette` (grid and value) and `zone_palette` (id and display
  name) of the view's plane. `--match TEXT` keeps entries whose id contains
  `TEXT`, ignoring case, exactly as the desktop Place palette filter does; it
  takes at most 64 bytes.
- `tilewright.editor-lint/1`: `counts` by severity and `findings`, each with a
  stable `code`, a `severity`, a `location` (some of `path`, `plane`,
  `layer`, `tile`, `uid`, `carried`, `anchor`, `zone`, `grid`, `codes`), a `message`
  this editor wrote and a `detail` quoting the loader when there is one.
- `tilewright.editor-load-failure/1`: `problems` and `diagnostics` (path,
  line, column, byte span, code and message), when the world does not load.
- `tilewright.editor-refusal/1` (with `--json` only): `code` and `message`.

## Lint findings

Lint reports what the editor's existing checks find, nothing more. World's
own diagnostics come with a world that does not load. Reachability is not
checked: the profile cannot yet say what blocks movement.

Severities are defined by the desktop editor's entry points:

- `open ROOT` and these read-only commands need only World to load the
  world; they do not read the catalog.
- `window` opens no world.
- The editing entry point, `prototype`, which the provisioning helper
  starts on a private copy, opens the world with the prototype load budget
  (`tilewright_editor_document::prototype_load_options`: at most 256 files,
  1,024 directory entries and 8 MiB read, 1 MiB per document), then requires
  `preview.jsonc` to load and the catalog's view to be prepared.

An **error** means the editing entry point cannot open or show the world as
it is; `lint` then exits 1. A **warning** is probably a mistake, but the
editing entry point opens the world. **Info** is worth knowing; nothing is
wrong.

| Code | Severity | Meaning |
|---|---|---|
| `world.over_editor_budget` | error | The prototype load budget refuses the world, which loads with World's default bounds. `location.codes` lists World's diagnostic codes for the budgeted load as World reported them, such as `source.budget` and the `source.missing_file` of the files it then left unread. |
| `catalog.absent` | error | The world has no `preview.jsonc`. The editing entry point cannot open it; `open ROOT` and these commands can. |
| `catalog.invalid` | error | `preview.jsonc` is there but does not load. |
| `preview.unprepared` | error | The catalog's view cannot be prepared for display, for example an object in it whose prototype the catalog cannot draw. |
| `preview.*` (others) | info | A note the preview adds when it prepares the view, such as `preview.effects-neutral`. |
| `portal.target_own_tile` | warning | A portal leads to the tile its object stands on. |
| `portal.target_no_chunk` | warning | A location target is in a map chunk that does not exist. |
| `portal.target_no_ground` | warning | A location target has no ground on the pick grid. |
| `anchor.unreferenced` | info | No portal names the anchor. |
| `zone.unused` | info | No tile, chunk palette or child zone uses the zone and it is not the plane default. |

The portal checks are the ones the desktop review shows when a portal is
edited; the pick grid is the first grid the catalog grants terrain editing
on, else the profile's first ground grid. A portal carried in an inventory
is checked from the tile of the placed object that carries it.

## Limits

Every list is bounded; a bounded list `name` is followed by `name_more`, the
number of items it left out. Free text longer than its bound is cut and
followed by `name_clipped: true`. The numbers live in
`crates/tilewright-editor-observe/src/limits.rs`:

| Bound | Value |
|---|---|
| Characters of one free-text value | 256 |
| Bytes of one printed report | 1,048,576 (1 MiB) |
| `overview`: planes | 32 |
| `overview`: spatial layers per plane | 16 |
| `overview`: render layers, terrains, view zones, entries of each grant list | 64 each |
| Field names, default names and granted fields per object or prototype; fields per object | 8 |
| `tile`: objects | 16 |
| `tile`: anchors on the tile; arrivals | 8 each |
| `catalog`: prototypes, terrain choices | 128 each |
| `catalog`: zones | 64 |
| `catalog`: `--match` | 64 bytes |
| `lint`: findings listed (all are counted) | 256 |
| Load failure: diagnostics listed (`problems` counts all, at most 256) | 128 |

Beyond loading the world, each command does work proportional to the world's
size at most. The bounds are chosen so that every report fits the output
budget at its worst: every list full, every id at World's longest and every
free-text value made of characters that print as 12-byte escapes. Tests
build exactly that; the largest JSON reports are a load failure (about
827 KB) and `tile` (about 437 KB). A report past the budget would be refused
(`output.budget`) rather than cut. There is no paging yet: `--match` narrows
`catalog`, and `tile` takes one tile at a time.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. For `lint`: no finding is an error. |
| 1 | `lint` only: at least one finding is an error. |
| 2 | Usage error: nothing is printed on stdout and the usage goes to stderr. |
| 3 | The world does not load: the load failure is the output. |
| 4 | Refused: `target.unknown` (the tile's plane does not exist) or `output.budget`. |

`lint` follows the world CLI's `diff` convention (0 clean, 1 findings, 2
usage) and adds 3 for a world that does not load, which every command shares.
