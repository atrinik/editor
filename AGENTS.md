# Tilewright editor repository guide

## Ownership and dependencies

- This repository owns the MIT native Rust world editor for isometric and
  top-down tile games:
  projects/documents, command history, panels/tools, document-to-scene
  adaptation, diagnostics UI, autosave/recovery, preview, and packaging.
- Consume released world toolkit crates for lossless syntax,
  schemas/catalogs, compilation, diagnostics, diffs, and atomic transactions.
  Never add a second parser, validator, serializer, or write path.
- Consume the released Tilewright renderer for scenes, GPU resources, viewport,
  semantic masks, picking, typography, and offscreen previews. Never fork
  renderer code or create editor-only projection, painter, lighting, cache, or
  pixel-picking behavior. One recorded exception (ADR 0001 amendment 1):
  where the frame's semantic mask holds no tile, as on the renderer's
  one-pixel seams, the pointer picks the nearest tile within `PICK_REACH`
  pixels, until the editor adopts the renderer's seam fix
  (tilewright-renderer #55), which is when to remove it.
- Do not make editor builds or runtime depend on game clients, servers,
  protocols, or game-specific code. Game specifics arrive as data through game
  profiles. Preview is local rendering; playable tests belong to each game's own
  tooling.
- Local renderer/toolkit overrides stay outside committed manifests; production
  manifests retain immutable released dependencies.

## Documents and filesystem safety

- Toolkit documents remain authoritative. UI state holds views/selections and
  semantic commands, not alternate serialized truth or direct file writers.
- Every edit carries preconditions and uses toolkit transactions so dry-run,
  diff, undo/redo, validation, and atomic publication share one path. A save
  expects the values it replaces; undo and redo expect the exact files their
  save left (`docs/MUTABLE_PATHS.md`).
- Preserve comments, unknown fields, ordering, whitespace, line endings,
  nesting, and untouched bytes. Opening/saving unchanged data is byte-identical
  and targeted edits do not churn unrelated source.
- Use explicit project roots and write allowlists. Canonicalize/revalidate at
  the operation boundary; reject traversal, symlink escape, special files,
  generated/runtime targets, ambiguous case, and paths changed after review.
- Detect external edits to what an edit changes, and stale revisions.
  Validation, conflict, disk, permission, rename, or crash failure leaves
  source unchanged and recoverable.
  Keep bounded autosave outside source and never execute project code.
- Keep GPU handles renderer-private and filesystem authority toolkit-owned.
  Document-to-scene adaptation is read-only/deterministic; selection uses exact
  semantic masks rather than color inference.
- Keep headless project/document/command/UI models independent of the
  window, GPU, network, and real filesystem. Isolate native/unsafe code
  narrowly.

## Licensing, delivery, and validation

- New code/tests/docs/editor fixtures are MIT. Do not link, bundle, depend on,
  copy, or adapt GPL/AGPL source. Independent implementation is the default;
  any reuse of outside material needs an MIT-compatible license and an exact
  record (source, revision, destination, transformation, notice) in
  [PROVENANCE.md](PROVENANCE.md) and `THIRD_PARTY_NOTICES.md`.
- `THIRD_PARTY_NOTICES.md` is generated from `Cargo.lock` by
  `tools/third-party-notices.py`; run it after any dependency change, and
  add notices for outside material other than crates to its template.
- Authored material keeps exact licenses. Preview/test assets require a
  source/author/license/digest/transformation/notice manifest; preserve legal
  metadata and fail on ambiguous inputs.
- Packages pin renderer/toolkit releases and carry checksums, SBOM, provenance,
  notices, and exact allowlisted assets. They must not bundle game content, a
  game server/client, source, or mutable state.
- Local issues and milestones own executable acceptance criteria.
- Prefer deterministic headless transaction/path/recovery tests and add
  malformed/large projects, path attacks, external edits, disk/permission
  failure, GPU loss, and repeated open/edit/save/reopen cases as relevant.
  Visual tests use the shared renderer, exact masks, explicit clocks/resources,
  and documented color tolerance.
- `policy/released-dependencies.json` lists the released renderer and toolkit
  crates and their source repositories for package provenance. Keep internal
  editor packages unpublished; application archives remain GitHub releases.
- Run the aggregate contract now present:

  ```sh
  tools/validate.sh
  git diff --check
  ```

  `Editor validation` owns formatting, strict Clippy, workspace tests/docs,
  dependency architecture/security/license gates, transaction faults,
  renderer/toolkit compatibility, and supported platform/package proofs.
- Use repository validation. Commits/PR titles use Conventional Commits;
  semantic-release owns releases/tags.
- Dependency review is advisory, including operational errors. Before merge, the
  implementing agent must inspect the latest run for the current PR head and
  read its job summary even when a bot comment exists, fix actionable introduced
  risk, or record explicit maintainer acceptance of existing risk. A comment
  from an earlier run is not current evidence; unavailable reports leave results
  unknown. Reporting applies to eligible PR runs where this workflow exists in
  the relevant PR/merge revision; removing branch filters does not backport it.
  License metadata reporting does not automate compatibility policy: existing
  repository license and provenance constraints remain authoritative. Validate
  workflow changes with actionlint and `git diff --check`.
