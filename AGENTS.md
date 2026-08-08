# Atrinik editor repository guide

## Ownership and dependencies

- This repository owns the fresh MIT native Rust authoring application:
  projects/documents, command history, panels/tools, document-to-scene
  adaptation, diagnostics UI, autosave/recovery, preview, and packaging.
- Consume released `atrinik/content-toolkit` crates for lossless syntax,
  schemas/catalogs, compilation, diagnostics, diffs, and atomic transactions.
  Never add a second parser, validator, serializer, or write path.
- Consume released `atrinik/renderer` for scenes, GPU resources, viewport,
  semantic masks, picking, typography, and offscreen previews. Never fork
  renderer code or create editor-only projection, painter, lighting, cache, or
  pixel-picking behavior.
- Do not depend on client sessions, GP1, Go internals, classic implementation,
  archived predecessors, or Gridarta. Preview is local rendering; playable
  tests belong to wrapper-owned isolated topologies.
- Local renderer/toolkit overrides use wrapper profiles; production manifests
  retain immutable released dependencies.

## Documents and filesystem safety

- Toolkit documents remain authoritative. UI state holds views/selections and
  semantic commands, not alternate serialized truth or direct file writers.
- Every edit carries document/file revision preconditions and uses toolkit
  transactions so dry-run, diff, undo/redo, validation, and atomic publication
  share one path.
- Preserve comments, unknown fields, ordering, whitespace, line endings,
  nesting, and untouched bytes. Opening/saving unchanged data is byte-identical
  and targeted edits do not churn unrelated source.
- Use explicit project roots and write allowlists. Canonicalize/revalidate at
  the operation boundary; reject traversal, symlink escape, special files,
  generated/runtime targets, ambiguous case, and paths changed after review.
- Detect external edits and stale revisions. Validation, conflict, disk,
  permission, rename, or crash failure leaves source unchanged and recoverable.
  Keep bounded autosave outside source and never execute project code.
- Keep GPU handles renderer-private and filesystem authority toolkit-owned.
  Document-to-scene adaptation is read-only/deterministic; selection uses exact
  semantic masks rather than color inference.
- Keep headless project/document/command/UI models independent of SDL3, GPU,
  network, and real filesystem. Isolate native/unsafe code narrowly.

## Licensing, delivery, and validation

- New code/tests/docs/editor fixtures are MIT. Do not add GPL/AGPL or adapt
  classic/Gridarta implementation. Historical reuse follows local
  `PROVENANCE.md` and canonical `atrinik/atrinik/docs/PROVENANCE.md`, failing
  closed on incomplete or mixed evidence.
- Authored material keeps exact licenses. Preview/test assets require a
  source/author/license/digest/transformation/notice manifest; preserve legal
  metadata and fail on ambiguous inputs.
- Packages pin renderer/toolkit releases and carry checksums, SBOM, provenance,
  notices, and exact allowlisted assets. They must not bundle Gridarta, a game
  server/client, Python, classic libraries, source, or mutable state.
- `atrinik/atrinik#168` is the program roadmap; local issues/milestones own
  executable acceptance criteria. Do not duplicate M1-M6 prose here.
- Prefer deterministic headless transaction/path/recovery tests and add
  malformed/large projects, path attacks, external edits, disk/permission
  failure, GPU loss, and repeated open/edit/save/reopen cases as relevant.
  Visual tests use the shared renderer, exact masks, explicit clocks/resources,
  and documented color tolerance.
- Run the aggregate contract now present:

  ```sh
  tools/validate.sh
  git diff --check
  ```

  `Editor validation` owns formatting, strict Clippy, workspace tests/docs,
  dependency architecture/security/license gates, transaction faults,
  renderer/toolkit compatibility, and supported platform/package proofs.
- Wrapper replacement build/runtime adapters are not available yet. Use
  repository validation, not classic fallbacks. Commits/PR titles use
  Conventional Commits; semantic-release owns releases/tags.
