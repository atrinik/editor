# Tilewright editor

Tilewright editor is an MIT-licensed Rust world editor for isometric and
top-down tile games, with built-in agent tooling. Humans and agents edit the
same world through one command engine: the desktop app, a headless CLI, and an
MCP server all produce the same reviewable changesets.

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
renderer owns scenes and GPU resources. The seven editor crates own application
composition, project/tab and safe path state, document adaptation,
commands/history, UI panels/tools, preview integration, and deterministic test
fakes. None owns a parser, serializer, filesystem writer, renderer, network
service, or server lifecycle.

The editor opens world source trees with tilewright-world's `tilewright-source`
and carries out edits as its `tilewright-edit` transactions; it builds scenes
with tilewright-renderer 0.3. All are exact crates.io releases, recorded with
their repositories in `policy/released-dependencies.json`.

## Build and validation

Rust 1.97.1 is pinned, and `Cargo.lock` records the resolved SDL bindings and
SDL source versions used by `--locked` builds. External dependency declarations
use compatible SemVer ranges so routine updates can refresh the lockfile
without editing a second version inventory. Linux builds need the native SDL
headers installed by `tools/install-linux-native-deps.sh`.

```sh
cargo build --locked --workspace
cargo test --locked --workspace --all-targets
cargo run --locked --package tilewright-editor -- version
cargo run --locked --package tilewright-editor -- open crates/tilewright-editor-testkit/fixtures/meadow
cargo run --locked --package tilewright-editor -- paint crates/tilewright-editor-testkit/fixtures/meadow --plane meadow --terrain sand 30,3 33,3
SDL_VIDEO_DRIVER=dummy cargo run --locked --package tilewright-editor -- window
tools/validate.sh
```

`tools/validate.sh` checks formatting, Clippy-as-errors, all tests/docs,
dependency architecture/licenses/advisories, provenance/assets, path/inventory
contracts, the shared-renderer empty viewport, SDL lifecycle, Linux release
dry-run/SBOM/reproducibility, and diff hygiene. GitHub also builds/tests/packages
on Windows and exposes one required aggregate check named `Editor validation`.

The editor never launches a game server; playtests are run by the consuming
game's own tooling.

## Editing and saving

Edits are world toolkit transactions. A session keeps the edits of the
running interaction in memory, where the preview shows them with the
profile's terrain transitions resolved, and saves them to the source tree
when the interaction ends: a pointer or key is released. Interactions that
begin within 2 seconds of the last saved one join its undo step, so a burst
of strokes undoes as one; undo publishes the toolkit's exact inverses of the
step's saves, and redo the transactions again. A save whose tiles changed
outside the editor is dropped and the world read again; a save that cannot be
published waits to be saved again or discarded. The `paint` command strokes
the terrain brush from the command line: it prints the transaction, or with
`--save INVERSE` keeps the inverse outside the source tree and then
publishes the change.

## Safety and releases

[ADR 0001](decisions/0001-editor-architecture.md), the
[threat model](docs/THREAT_MODEL.md), and [mutable path table](docs/MUTABLE_PATHS.md)
define authority.

Every squash merge uses a Conventional Commit PR title and semantic-release.
Linux and Windows packages include checksums, CycloneDX SBOMs, provenance,
license, and third-party notices; no game content, server or client, recovery
data, or mutable project state is bundled.
