# Atrinik editor

This is the fresh MIT Rust authoring application for Atrinik. It is independent
of Gridarta, `atrinik/classic`, the connected client, protocol, and server.

## Development model

The editor is part of Atrinik's agentic next-generation reimplementation. Its
fresh MIT-licensed Rust code is developed primarily through OpenAI Codex
workflows under maintainer direction and review, with clean-room provenance
controls, tests, and repository validation. This describes the project's
current primary development workflow, not every line, commit, or contributor;
direct human-written code contributions are welcome.

This is a clean-room reimplementation and improvement of Classic/Gridarta-era
authoring workflows, not a mechanical translation or source port. Follow the
[replacement roadmap](https://github.com/atrinik/atrinik/issues/168) and the
[canonical project authorship statement](https://atrinik.org/licenses/) for
the wider project boundaries.

The editor operates on maps, quests, lore, archetypes, pixel art, and other
content owned and directed by human creators. The editor's MIT license does not
change that content's authorship, provenance, or license, and the editor must
not add a generative-content feature that conflicts with the owning content
repository's policy.

## M1 architecture

The editor pins the immutable v1.0.0 source revisions of
[`atrinik/content-toolkit`](https://github.com/atrinik/content-toolkit) and
[`atrinik/renderer`](https://github.com/atrinik/renderer). Toolkit documents and
transactions remain authoritative; renderer scenes and GPU resources remain
renderer-owned. Wrapper profiles are the only supported local override
mechanism.

```text
released content toolkit -> document adapter -> semantic commands/history
                                      |                    |
project/path policy ------------------+------> UI model ----+-> app
released renderer -------> scene/preview adapter ----------+
```

The seven workspace crates own application composition, project/tab and safe
path state, document adaptation, commands/history, UI panels/tools, preview
integration, and deterministic test fakes. None owns a parser, serializer,
filesystem writer, renderer, network service, or server lifecycle.

## Build and validation

Rust 1.97.1 and SDL 3.4.14 are pinned. Linux builds need the native SDL headers
installed by `tools/install-linux-native-deps.sh`. The Atrinik devcontainer
release that includes the editor toolchain also provides them.

```sh
cargo build --locked --workspace
cargo test --locked --workspace --all-targets
cargo run --locked --package atrinik-editor -- version
cargo run --locked --package atrinik-editor -- headless
SDL_VIDEO_DRIVER=dummy cargo run --locked --package atrinik-editor -- window
tools/validate.sh
```

`tools/validate.sh` checks formatting, Clippy-as-errors, all tests/docs,
dependency architecture/licenses/advisories, provenance/assets, path/inventory
contracts, the shared-renderer empty viewport, SDL lifecycle, Linux release
dry-run/SBOM/reproducibility, and diff hygiene. GitHub also builds/tests/packages
on Windows and exposes one required aggregate check named `Editor validation`.

The root wrapper does not yet expose replacement editor build/run commands;
that handoff belongs to `atrinik/atrinik#266`. It must use profile-local Cargo
overrides rather than editing these manifests. The editor never launches a
server; future playtests use wrapper-owned profile/topology/state lifecycle.

## Safety, parity, and releases

[ADR 0001](decisions/0001-editor-architecture.md), the
[threat model](docs/THREAT_MODEL.md), and [mutable path table](docs/MUTABLE_PATHS.md)
define authority. The machine-readable
[behavior inventory](migration/behavior-parity.json) assigns every observed
classic authoring workflow to one owner/issue/milestone and requires fixture
evidence before parity claims.

Every squash merge uses a Conventional Commit PR title and semantic-release.
Linux and Windows packages include checksums, CycloneDX SBOMs, provenance,
license, and third-party notices; no content, server/client, classic code,
Gridarta, recovery data, or mutable project state is bundled.
