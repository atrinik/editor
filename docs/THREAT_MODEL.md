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
  replacement, permission change, or path retarget produces a conflict. A brush
  save is the exception for content: it expects the values of the cells it
  paints, so an outside edit to other cells of a file it changes is kept rather
  than refused (`docs/MUTABLE_PATHS.md`).
- Preview validation/diffs before publication. Multi-file publication is one
  toolkit transaction; disk-full, permission, rename, crash, and validation
  faults publish all intended revisions or none. An undo step that coalesced
  several saves is several transactions: each is all or none, and a fault
  partway splits the step, leaving the part not yet undone or redone in place;
  the error says why the failing save could not be published, and the
  history shows the split.
- Keep bounded recovery journals outside the project. Recovery is explicit,
  visible, expiring, and never silently written over source.
- On Windows, never let a project's folder supply a DLL. The GPU libraries
  load some by name (`dxcompiler.dll`, `vulkan-1.dll`), and Windows searches
  the current directory for them, so `edit` resolves its paths and then moves
  the process into the editor's own folder before any GPU library loads, and
  pins eframe's DX12 shader compiler to FXC, which ships with Windows. The
  renderer's instance keeps wgpu's default compiler choice, as
  tilewright-renderer 0.3 offers no option for it; the directory change
  covers it. Renderer pin pending: a later renderer release defaults its
  instance to FXC on Windows, and the editor adopts it then, keeping the
  directory change.
- Never execute project scripts, shell fragments, plugins, macros, or build
  instructions merely by opening a project. Automation is an explicit bounded
  command with dry-run defaults.
- Never connect to, start, embed, stop, or mutate a game server. Playtest is a
  versioned wrapper request with isolated state and cleanup.

## Current M1 proof and residual work

`RelativePath`, `PathPolicy`, `FileStamp`, and `PathProbe` enforce lexical,
class, canonical, type, identity, and revision checks with deterministic fakes.
They intentionally perform no real filesystem operation. Canonical paths use a
slash-normalized adapter contract. Issue #4 integrates the released toolkit
publisher and OS-specific canonical/file-identity/case-collision adapter; issue
#9 owns fault, race, symlink-swap, permission, disk-full, and crash tests.
