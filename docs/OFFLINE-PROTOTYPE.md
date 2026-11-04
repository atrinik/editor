# Offline editing prototype

The prototype composes released World 0.1.0 and renderer 0.5.1. The generic
editor opens ordinary World data and its strict `preview.jsonc` catalog. A
separate game-specific toolkit adapter imports a selected external map and
exports a checked subset of edits. The editor contains no game-specific map
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
  --editor target/debug/tilewright-editor
```

A Linux binary archive includes the same trusted helper and authored sample.
From the extracted archive, use Python 3 (standard library only):

```sh
python3 tools/provision-prototype.py --world samples/prototype-neutral \
  --destination /absolute/private/new-session --editor bin/tilewright-editor
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

The native window presents the prepared scene through the released wgpu/SDL3
adapter and remains open until the user closes it. Window title and terminal
show state, selection, help, diagnostics and the actual planned source diff.
Selection uses exact reference-renderer semantic masks for the matching frame;
terrain mode uses a separate opaque cell-reference scene. SDL logical pointer
positions convert to physical pixels once. Resize/camera/session changes
invalidate earlier masks.

Planned edits are visible before acceptance. Accept retains the exact inverse
before World publication; Cancel publishes nothing. Undo and redo prepare World
transactions and require acceptance through the same path. An uncertain
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
preparation as interactive input. It is a deterministic scripted demonstration,
not a claim of human input. `--reference` produces CPU reference evidence and
must be reported separately from an actual visible native window run. Native
presentation/backend/adapter evidence cannot be inferred from passing unit tests
or CPU frames. Screenshot/video artifacts must identify which path produced
them and the exact source revision and dependencies.

Packages include only allowlisted authored MIT sample data/resources, immutable
registry references, notices, checksums, SBOM and provenance. Imported worlds,
real game maps/art, adapter baseline, recovery records and session evidence
are excluded. Windows build/test/package success is not Windows durable-editing
or native D3D12 qualification.

Later work includes rich panels/text, arbitrary-root provisioning APIs,
automatic crash replay, persistent undo, additional external-map edit forms,
original art/effects qualification, broader platform durability and
game-runtime cutover. Each needs its own evidence and authority; none is
implied by this prototype.
