# Offline editing prototype

The prototype targets World 0.1.0 and renderer 0.6.1. Release builds require
the exact published registry dependencies; local source candidates do not
establish released-consumer acceptance. The generic
editor opens ordinary World data and its strict `preview.jsonc` catalog. A
separate Atrinik toolkit adapter imports a selected external Classic map and
exports a checked subset of edits. The editor contains no Classic parser and
never starts commands selected by project data.

This is a schematic editor demonstration, with original MIT geometric art.
It does not claim Classic artwork, gameplay or visual parity. Classic remains
the active game content/runtime. Original map bytes, licensing and import
baseline belong to the external adapter workspace and are never bundled here.

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
Move or Terrain mode and exposes Clear, Undo and Redo. The inspector shows
the selected object's identity, position and schema fields, or the selected
cell and explicit terrain choices. Only profile-authorized scalar fields are
editable; structured values remain read-only.

Click an editable field or position, type or backspace at the working end,
then choose Review. Long focused input keeps its tail and caret visible.
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

Planned edits are visible before acceptance. Accept retains the exact inverse
before World publication; Cancel publishes nothing. Undo and redo prepare World
transactions and require acceptance through the same path. A pending plan must
be accepted or cancelled before another edit, undo or redo is prepared. Valid
semantic selections survive acceptance. Native input cannot accept a pending
change before its current frame has been presented. An uncertain
applied/durable/cleaned result blocks further editing and preserves its record.
Reopen is explicit inspection/reload, never blind recovery or retry.

The first Classic edit moves an eligible UID-bearing top-level object only
when its original block has one existing `x` and `y`, within original bounds.
Same-chunk coordinate changes use guarded generic World path transactions to
preserve source object order. Cross-chunk relocation still uses World's
`object.move` and may exceed the adapter's export allowlist; the exporter refuses
unsupported structural/order changes. Other fields, ordering and bytes are
preserved by the adapter's checked export.
The neutral sample also exposes a typed field and terrain paint operation.
Classic floors remain objects; no generic terrain capability is inferred.

Export is a separate explicit toolkit CLI operation on committed World data.
Pending scenes are never export input. Keep the original content tree read-only
and export into a new task-owned output. See the adapter's own usage and tests
for its manifest/stale-source/unsupported-edit rejection contract.

## Evidence and limits

`--demo-script` drives the same session actions, command history and scene
preparation as interactive input. It is a deterministic scripted demonstration,
not a claim of human input. `--reference` produces composed workspace CPU
reference evidence and
must be reported separately from an actual visible native window run. Native
presentation/backend/adapter evidence cannot be inferred from passing unit tests
or CPU frames. Screenshot/video artifacts must identify which path produced
them and the exact source revision and dependencies.

Packages include the exact allowlisted OFL Unicode font pack and authored MIT sample data/resources, immutable
registry references, notices, checksums, SBOM and provenance. Imported worlds,
real Classic maps/art, adapter baseline, recovery records and session evidence
are excluded. Windows build/test/package success is not Windows durable-editing
or native D3D12 qualification.

Later work includes a full widget/text-editing system, arbitrary-root provisioning APIs,
automatic crash replay, persistent undo, additional Classic edit forms, original
art/effects qualification, broader platform durability and game-runtime cutover.
Each needs its own evidence and authority; none is implied by this prototype.

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

### Private shared inspector candidate

`prototype --shared-inspector` opts in to a single shared string-value field in
this same inspector sidebar. Pass `--shared-inspector` to
`tools/provision-prototype.py` to forward it through the normal trusted launcher. Other field kinds use the existing editor input.
Click an editable string row (the neutral fixture's `label` is the acceptance
example). Shared **Apply** prepares the existing source review. Only the normal
review **Accept** publishes the World transaction; Cancel/Discard never writes
source. Text/schema/glyph/stale-binding errors retain the draft. Switching the
selection or discarding the field retires its native input and queued actions.

This source candidate requires the separately reviewed, unpublished
`tilewright-ui`, `tilewright-app`, and `tilewright-ui-render` 0.1.0 APIs. Its
registry version declarations are release-order placeholders: it is not a
resolvable production release graph until those crates are published. Private
validation uses copied sources, external Cargo patches and a generated snapshot
lock; no path/git override belongs in the production manifests or lock. Native
embedding, packaging, and final registry-only acceptance remain separate gates.
