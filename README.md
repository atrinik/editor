# Tilewright editor

Tilewright editor is an MIT-licensed Rust world editor for isometric and
top-down tile games, designed so that agents can drive it later. Humans and
agents are meant to edit the same world through one command engine. The bounded offline prototype provides
a native toolbar, scalar inspector, coordinate tools, a terrain drag brush
with a live preview of the cells a stroke will change,
object placement from a searchable palette of granted prototypes with thumbnails, object deletion,
a zone overlay with a legend and a zone brush, zone management (display name, parent, new zones, deleting unused ones),
anchor and portal badges with an anchor list, anchor editing (create, move, delete anchors no portal names), portal editing one object at a time (an anchor from a list, a tile picked in the view, a typed location, or none), reviewed changes and undo/redo. Deterministic reference capture accompanies native
window testing; an editor MCP server and a full widget system remain future work.

Tilewright is maintained by Seraphina Studios alongside
[tilewright-world](https://github.com/seraphinastudios/tilewright-world), the
world format and toolkit, and
[tilewright-renderer](https://github.com/seraphinastudios/tilewright-renderer),
the shared GPU renderer. Each game describes itself to the editor through a
game profile (object schema, layers, projection, movement rules, palettes), so
the editor carries no game-specific code.

## Development model

Development is primarily agent-driven, under maintainer direction, review,
provenance controls, tests, and repository validation. Direct human-written
code contributions are welcome under the same controls. See
[provenance](PROVENANCE.md) for the reuse record.

Maps, quests, art, and other content opened in the editor belong to each game,
with their own credits and licenses; they are not part of the editor's
MIT-licensed code.

## Architecture

```text
world toolkit release -> document adapter -> semantic commands/history
                                    |                    |
project/path policy ----------------+------> UI model ----+-> app
renderer release ------> scene/preview adapter ----------+
```

The world toolkit owns source syntax, lossless documents, schemas, catalogs,
diagnostics, compilers, semantic diffs, and atomic file transactions. The
renderer owns scenes and GPU resources. The nine editor crates own application
composition, project/tab and safe path state, document adaptation,
commands/history, UI panels/tools, preview integration, read-only observation
reports, and deterministic test fakes. None owns a parser, serializer, filesystem writer, renderer, network
service, or server lifecycle.

The editor opens world source trees with tilewright-world's `tilewright-source`
and carries out edits as its `tilewright-edit` transactions; it builds scenes
with tilewright-renderer 0.7.1, including its shared UI/text compiler.
The production manifests target exact crates.io
versions, recorded with their repositories in `policy/released-dependencies.json`.
The registry lockfile records their actual archive checksums; producer path/Git
overrides are not part of builds or package provenance.

The editor owns panel layout, focus and actions; glyph layout and drawing stay
in the shared renderer. The bundled common-Latin font preserves its OFL notice;
the native editor also requires the explicit Unicode font pack (`--font-pack ROOT`),
which packages carry with its original OFL notices.
Unsupported text stays intact and is protected against lossy interactive edits.
See [the prototype controls and limits](docs/OFFLINE-PROTOTYPE.md).

`tilewright-editor-commands::History` provides semantic transaction preparation,
reviewable file diffs, explicit commit, and undo/redo. It checks the opened world
revision and inverse plannability, retains a durable recovery record through the
application's `RecoveryStore` before publication, and delegates every source
write to World. See [the transaction contract](decisions/0001-editor-architecture.md)
for required recovery integration and uncertain-publication handling.

## Build and validation

Rust 1.97.1 is pinned, and `Cargo.lock` records the resolved SDL bindings and
SDL source versions used by `--locked` builds. External dependency declarations
use compatible SemVer ranges so routine updates can refresh the lockfile
without editing a second version inventory. Linux builds need the native SDL
headers installed by `tools/install-linux-native-deps.sh`.

The [offline prototype guide](docs/OFFLINE-PROTOTYPE.md) describes the authored
neutral sample, safe fresh session provisioning, controls, external adapter
workflow and Linux checked-editing limits. A game map remains external data;
its first preview uses authored schematic resources.

```sh
cargo build --locked --workspace
cargo test --locked --workspace --all-targets
cargo run --locked --package tilewright-editor -- version
cargo run --locked --package tilewright-editor -- open crates/tilewright-editor-testkit/fixtures/meadow
cargo run --locked --package tilewright-editor -- headless crates/tilewright-editor-testkit/fixtures/meadow
SDL_VIDEO_DRIVER=dummy cargo run --locked --package tilewright-editor -- window
python3 tools/provision-prototype.py --world crates/tilewright-editor-testkit/fixtures/prototype-neutral --destination /absolute/private/new-session --editor target/debug/tilewright-editor
tools/validate.sh
```

## Read-only commands

`overview`, `tile`, `catalog` and `lint` describe a world without a window or
an editing session (tested on Linux and Windows), as text or, with `--json`, as one
bounded JSON document. World text in their output is data, escaped in both
forms. See [the read-only commands](docs/CLI.md) for the output shapes,
limits and exit codes.

```sh
cargo run --locked --package tilewright-editor -- overview crates/tilewright-editor-testkit/fixtures/prototype-neutral
cargo run --locked --package tilewright-editor -- tile --json crates/tilewright-editor-testkit/fixtures/prototype-neutral main 0 7 0
cargo run --locked --package tilewright-editor -- catalog --match grass crates/tilewright-editor-testkit/fixtures/prototype-neutral
cargo run --locked --package tilewright-editor -- lint crates/tilewright-editor-testkit/fixtures/prototype-neutral
```

`tools/validate.sh` checks formatting, Clippy-as-errors, all tests/docs,
dependency architecture/licenses/advisories, provenance/assets, path/inventory
contracts, the shared-renderer empty viewport, SDL lifecycle, Linux release
dry-run/SBOM/reproducibility, and diff hygiene. GitHub also builds/tests/packages
on Windows and exposes one required aggregate check named `Editor validation`.

The editor never launches a game server; playtests are run by the consuming
game's own tooling.

## Safety and releases

[ADR 0001](decisions/0001-editor-architecture.md), the
[threat model](docs/THREAT_MODEL.md), and [mutable path table](docs/MUTABLE_PATHS.md)
define authority.

Every squash merge uses a Conventional Commit PR title and semantic-release.
Linux and Windows packages include checksums, CycloneDX SBOMs, provenance,
license, and third-party notices; no game content, server or client, recovery
data, or mutable project state is bundled. The original MIT neutral sample is
included through an exact data/resource allowlist.
