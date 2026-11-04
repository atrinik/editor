# Safe authoring threat model

## Assets and trust boundaries

Authored projects, paths, links, source bytes, metadata, automation
instructions, external changes, and renderer resources are untrusted. Source
documents and their legal attribution are the protected assets. Credentials,
runtime state, collected/generated output, and recovery data must never enter a
source transaction.

The editor holds presentation and command intent. Content-toolkit holds parsing
and transaction authority. Renderer holds GPU/resource authority. The root
wrapper alone may provision or supervise a server playtest.

## Required controls

- Accept portable relative slash-separated paths only. Reject absolute, drive,
  UNC, traversal, empty/dot segments, control characters, Windows alternate
  streams and device names, trailing dot/space aliases, excessive length, and
  ambiguous case.
- Declare source/write roots and deny generated, collected, build, runtime,
  state, log, and recovery prefixes even when nested below a source root.
- Canonicalize without following authority from project instructions. Reject
  symlinks and special files. Require canonical root containment.
- Capture file identity and content revision at inspection, then canonicalize
  and revalidate both immediately before toolkit publication. An external edit,
  replacement, permission change, or path retarget produces a conflict.
- Preview validation/diffs before publication. Multi-file publication is one
  toolkit plan with per-file atomic replacement and ordinary-error rollback;
  crash atomicity across all files is not promised. Preserve exact recovery
  and expose applied/durable/cleaned outcomes on uncertain failures.
- Keep bounded recovery journals outside the project. Recovery is explicit and
  visible and never silently written over source. The prototype retains records
  without automatic expiry, deletion or replay.
- Never execute project scripts, shell fragments, plugins, macros, or build
  instructions merely by opening a project. Automation is an explicit bounded
  command with dry-run defaults.
- Never connect to, start, embed, stop, or mutate a game server. Playtest is a
  versioned wrapper request with isolated state and cleanup.

## Historical M1 proof and residual work

`RelativePath`, `PathPolicy`, `FileStamp`, and `PathProbe` enforce lexical,
class, canonical, type, identity, and revision checks with deterministic fakes.
They intentionally perform no real filesystem operation. Canonical paths use a
slash-normalized adapter contract. Issue #4 integrates the released toolkit
publisher and OS-specific canonical/file-identity/case-collision adapter; issue
#9 owns fault, race, symlink-swap, permission, disk-full, and crash tests.

## Bounded offline prototype

The native application now composes World-backed Session/History, strict generic
catalog loading, immutable digest-verified resources, shared-renderer scenes,
reference semantic masks and wgpu/SDL presentation. It opens no game-specific
code and invokes no project executables. A separate game toolkit owns external
map import/export and the exact allowable source-span edits.

Editing requires trusted fresh Linux provisioning: distinct private World,
recovery and evidence directories, persistent supported local backing verified
from retained-FD mount IDs/device identity, synced creation and retained roots.
World's checked filesystem policy alone is insufficient because it includes
volatile tmpfs; the provisioner rejects tmpfs/ramfs/overlay/unknown backing.
No arbitrary user-supplied root gets an equivalent isolation/persistence claim.
Same-UID hostile writers, hostile root, physical-storage attestation and automatic
crash replay remain outside the bounded prototype guarantee.

The document retains explicit source budgets through reload and every planned
transaction. Catalog/resource changes invalidate input even when the World
revision is unchanged. Native logical input scales once into physical pixels;
stale frame generations, missing resources, failed scene preparation or renderer
failure cannot authorize a new edit. [The prototype guide](OFFLINE-PROTOTYPE.md)
records controls, pending review, retention, uncertainty and platform limits.
