# Read-only commands

The `tilewright-editor` binary answers four read-only questions about a world
without opening a window or an editing session: `overview`, `tile`,
`catalog` and `lint` (issue #57). The MCP server's observe tools will return
the same reports later. They run on Linux, macOS and Windows; editing stays
Linux-only.

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
at most once; an argument starting with `--` is always an option. `LAYER` is
a spatial layer (32-bit) and `X Y` are plane tile coordinates (64-bit).

## Read-only by construction

The commands open the world with `WorldDocument::open` (World's default load
bounds) and its editor catalog, `preview.jsonc`, with the preview loader.
Nothing else is opened: no session, recovery store, provisioned copy,
transaction or publication. The crate that builds the reports,
`tilewright-editor-observe`, never builds a `Session` and does not depend on
the transaction, history or recovery crates. A test runs every command in
both forms on a copy of the neutral sample and checks that every file, its
bytes, its permissions and its modification time are unchanged and that no
entry appears beside the world.

## World text is data

Ids, names, titles, versions, field values and loader messages come from
the world. In both forms they are values, never member names or structure:

- `--json` prints one JSON document on stdout. Every control character (C0,
  DEL and C1), the line and paragraph separators and the bidirectional
  formatting characters are written as `\u` escapes, so the document is safe
  to print as it is and parses with any strict JSON parser. Integers are JSON
  integers.
- Without `--json`, the same document prints as indented `name: value` lines.
  Each value is one escaped field (`terminal_field`): a world value cannot add
  or forge a line, move the cursor, retitle the terminal or reorder text.

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
  the tile bounds `[x, y, width, height]` its chunks cover), and `catalog`:
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
  `layer`, `tile`, `uid`, `carried`, `anchor`, `zone`, `grid`), a `message`
  this editor wrote and a `detail` quoting the loader when there is one.
- `tilewright.editor-load-failure/1`: `problems` and `diagnostics` (path,
  line, column, byte span, code and message), when the world does not load.
- `tilewright.editor-refusal/1` (with `--json` only): `code` and `message`.

## Lint findings

Lint reports what the editor's existing checks find, nothing more. World's
own diagnostics come with a world that does not load. Reachability is not
checked: the profile cannot yet say what blocks movement.

| Code | Severity | Meaning |
|---|---|---|
| `catalog.invalid` | error | `preview.jsonc` is there but does not load; the desktop editor cannot open the world. |
| `preview.unprepared` | error | The catalog's view cannot be prepared for display, for example an object in it whose prototype the catalog cannot draw. |
| `preview.*` (others) | info | A note the preview adds when it prepares the view, such as `preview.effects-neutral`. |
| `portal.target_own_tile` | warning | A portal leads to the tile its object stands on. |
| `portal.target_no_chunk` | warning | A location target is in a map chunk that does not exist. |
| `portal.target_no_ground` | warning | A location target has no ground on the pick grid. |
| `anchor.unreferenced` | info | No portal names the anchor. |
| `zone.unused` | info | No tile, chunk palette or child zone uses the zone and it is not the plane default. |

The portal checks are the ones the desktop review shows when a portal is
edited; the pick grid is the first grid the catalog grants terrain editing
on, else the profile's first ground grid.

## Limits

Every list is bounded; a bounded list `name` is followed by `name_more`, the
number of items it left out. Free text longer than its bound is cut and
followed by `name_clipped: true`. The numbers live in
`crates/tilewright-editor-observe/src/limits.rs`:

| Bound | Value |
|---|---|
| Characters of one free-text value | 256 |
| Bytes of one printed report | 1,048,576 (1 MiB) |
| `overview`: planes, spatial layers per plane, terrains | 64 each |
| `overview`: view zones; entries of each grant list | 64 each |
| `tile`: objects; fields and granted fields per object | 16 each |
| `tile`: anchors on the tile | 8 |
| `tile`: arrivals | 8 |
| `catalog`: prototypes, terrain choices, zones | 256 each |
| `catalog`: `--match` | 64 bytes |
| `lint`: findings listed (all are counted) | 256 |
| Load failure: diagnostics | 256 (World's own cap) |

Beyond loading the world, each command does work proportional to the world's
size at most. A report past the output budget is refused (`output.budget`)
rather than cut; the list bounds keep every report well below it.

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
