# Atrinik editor repository guide

## Mission and ownership

- This repository owns the fresh MIT-licensed native Rust authoring
  application: project discovery, multi-document application state, command
  history, panels and tools, document-to-scene adaptation, diagnostics
  presentation, autosave/recovery UI, preview integration, and editor packaging.
- Consume versioned releases of `atrinik/content-toolkit` for syntax, lossless
  documents, schema/catalog/compiler services, diagnostics, diffs, and atomic
  transactions. Never duplicate a content parser, validator, serializer, or
  file-transaction implementation in an editor crate.
- Consume versioned releases of `atrinik/renderer` for scene types, GPU
  resources, viewport rendering, semantic masks, selection/picking, typography,
  and offscreen previews. Do not fork renderer code or implement editor-only
  projection, painter order, lighting, resource caches, or pixel picking.
- Do not depend on `atrinik/client`, generated game protocol, client sessions,
  Go server internals, a legacy repository, or Gridarta. The editor is an
  offline authoring consumer, not a mode embedded into the connected client.
- Coordinated local renderer/toolkit overrides belong in an
  `atrinik/atrinik` wrapper profile and must not modify Cargo manifests.
  Production dependencies use immutable released versions.
- The editor never starts, embeds, mutates, or directly controls a game server.
  Preview is local rendering; playable tests use versioned wrapper commands
  with isolated build views, topology, state, port, client configuration, and
  cleanup.

## Documents, commands, and filesystem safety

- Keep toolkit documents authoritative. Editor tabs and panels hold views,
  selections, transient form state, and commands; they do not own alternate
  serialized truth or write files directly.
- Every edit is a semantic command with document/file revision preconditions.
  Route single- and multi-file changes through toolkit transactions so dry-run,
  diff, undo/redo, validation, and atomic publication share one implementation.
- Preserve unknown fields, comments, ordering, multiline text, line endings,
  nesting, and untouched bytes. Opening and saving an unchanged document is
  byte-identical; a targeted edit must not churn unrelated source.
- Define explicit project source roots and write allowlists. Canonicalize and
  revalidate paths at the operation boundary; reject traversal, symlink escape,
  generated/collected outputs, mutable runtime state, ambiguous case, special
  files, and paths changed after inspection.
- Detect external edits and stale revisions before writing. Failed validation,
  conflict, disk-full, permission, rename, or crash paths leave source files
  unchanged and retain an explicit recoverable state.
- Store autosave/recovery outside the source tree with bounded retention and
  clear user control. Never silently overwrite source, follow untrusted project
  instructions, execute project code, or write to a running/remote server.
- Keep GPU handles renderer-private and filesystem authority toolkit-owned.
  Document-to-scene adapters are read-only and deterministic; editor selection
  uses renderer semantic identity/depth/coverage masks, not reverse-engineered
  color pixels.
- Keep headless project/document/command/UI-model behavior independent of SDL3,
  GPU, network, and real filesystem where fakes can express the contract.
  Isolate unavoidable SDL3/native FFI in the smallest reviewed application
  boundary.

## Roadmap and issue discipline

- The master replacement plan is `atrinik/atrinik#168`; repository issues and
  acceptance criteria are the executable source of truth. Link every change to
  an issue and its M1-M6 milestone. Preserve authored-content and gameplay
  design choices; this program replaces technical implementation, not product
  intent.
- M1 establishes the clean-room Cargo application, dependency directions,
  safe-filesystem threat model, released renderer/toolkit boundaries, and the
  machine-readable classic authoring behavior inventory in issue #15. That
  inventory supplies editor capability IDs and evidence to the program-wide
  equivalence contract in `atrinik/atrinik#279`.
- M2 integrates lossless authored documents with the shared renderer using
  deterministic adapters and resource providers. It must not add a parser or
  editor-specific renderer.
- M3 delivers only the bounded vertical slice in issue #17: open and render one
  licensed map through the shared toolkit/renderer path, select one object,
  make one preconditioned semantic edit, validate and save atomically, reopen
  it losslessly, and invoke the wrapper-owned isolated playtest. Retain this
  path for M4; do not turn it into a throwaway parser, renderer, writer, or the
  full map-editor MVP.
- M4 delivers project/multi-document lifecycle, transactional commands,
  undo/redo, minimal atomic saves, shared-renderer viewport and map tools,
  catalog/tree/inspector/diagnostics, automation/preview/recovery, isolated
  playtests, and the map-editing MVP.
- M5 adds focused non-map panels only from measured authoring demand and on the
  same toolkit transaction/model contracts, then issue #16 burns the M1
  inventory down across the complete supported content pack. Publish its
  machine-readable parity evidence for the aggregate cutover gate in
  `atrinik/atrinik#280`.
- M6 owns fuzz/fault/soak/recovery, Linux/Windows packages, final Gridarta
  replacement evidence, and supported-workflow cutover.
- Project shell, command/history model, panels, document adapters, viewport
  tools, diagnostics presentation, and packaging may proceed in parallel after
  issue #2 freezes ownership and API directions. Use fakes or reviewed released
  contracts; never unblock work by copying toolkit, renderer, or client source.

## Licensing, provenance, and authored material

- New Rust code, tests, documentation, and editor-specific fixtures in this
  repository are MIT. Do not add GPL/AGPL code dependencies or adapt Gridarta,
  legacy editor/client/server source, tests, comments, or internal structure by
  default. Observable workflows and preserved product specifications may guide
  an independent implementation.
- Historical reuse is allowed only for a person and scope present in the
  exhaustive approved-grantor registry in the current `atrinik/atrinik`
  `AGENTS.md`. Apply its complete-history, identity, separability,
  third-party-review, and recording requirements exactly; fail closed on any
  incomplete history, mixed authorship, uncertain origin, or conflicting
  notice. Cite the exact wrapper revision containing the registry entry in the
  destination pull request or provenance manifest.
- Authored maps, archetypes, graphics, fonts, audio, attribution metadata, and
  other project material keep their exact individual licenses. Editing,
  previewing, testing, or packaging a project does not relicense it, and this
  repository's MIT license does not cover a mixed content tree.
- Test/preview assets require a machine-readable manifest with source, author,
  exact license, digest, transformation, and required notice. Review
  derivatives/composites against every input; fail on ambiguous, incompatible,
  missing, or unacknowledged material.
- Preserve attribution fields and unknown licensing metadata losslessly. The
  editor may diagnose missing metadata but must not silently synthesize or
  replace legal attribution.

## Rust quality and validation

- Pin stable Rust, edition, MSRV policy, SDL3/UI/native acquisition, and the
  application `Cargo.lock`. Keep dependencies minimal, audited,
  license-compatible, and represented in the wrapper supply-chain inventory
  before relying on them.
- Once Cargo exists, every change must pass the aggregate `Editor validation`
  contract: rustfmt, Clippy with warnings denied, workspace unit/integration/doc
  tests, dependency-architecture tests, dependency/license/security checks,
  lossless/transaction fault tests, renderer/toolkit compatibility, and
  applicable Linux and Windows builds and packaging dry-runs.
- Prefer deterministic headless tests for project state, commands, undo/redo,
  external-change handling, diagnostics links, path policy, and transaction
  recovery. Add malformed/large projects, path/symlink attacks, permission and
  disk failures, GPU/device loss, and repeated open/edit/save/reopen cases as
  their foundations land.
- Visual tests must use the released shared renderer, explicit clocks and
  resources, exact semantic masks, and documented color tolerances. Do not
  accept a screenshot-only test as proof of selection, depth, or disclosure.
- Treat warnings as errors. Avoid network access, ambient user state,
  source-tree mutation, nondeterministic file watchers/clocks, and tests that
  require sibling source checkouts. Always run `git diff --check`.
- Use the thin wrapper whenever it supports the fresh editor. Cross-repository
  handoffs identify an exact profile and run the wrapper build/test contract.
  Playtest handoffs must include `topology show`, `up`, `ps`, relevant `logs`,
  expected edit/validate/play actions, and `down`, with unique topology and
  state names and no direct executable/server invocation.

## Packages, releases, and current repository state

- This repository independently owns the `atrinik-editor` executable and its
  Linux/Windows packages. Packages pin compatible renderer/toolkit releases and
  include checksums, SBOM, provenance, MIT/dependency notices, and only exact
  allowlisted assets. They must not bundle Java/Gridarta, a server/client,
  Python, legacy libraries, source checkouts, or mutable game state.
- Pull-request titles and squash commits use Conventional Commits. Every squash
  merge is released by semantic-release; do not create tags manually or couple
  publication to wrapper, renderer, toolkit, or client commits.
- The repository is currently a seed containing only licensing and roadmap
  documentation. Until issue #1 lands Cargo and CI, do not claim that rustfmt,
  Clippy, tests, shared viewport, toolkit integration, packaging, or runtime
  validation ran. For seed-only documentation changes, inspect the complete
  tree, confirm the MIT boundary and links, and run `git diff --check`; report
  all unavailable future checks honestly. After bootstrap, the
  repository-defined full validation is mandatory.
