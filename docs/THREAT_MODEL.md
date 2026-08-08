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

- Accept relative slash-separated paths only. Reject absolute, drive, UNC,
  traversal, empty/dot segments, NUL, excessive length, and ambiguous case.
- Declare source/write roots and deny generated, collected, build, runtime,
  state, log, and recovery prefixes even when nested below a source root.
- Canonicalize without following authority from project instructions. Reject
  symlinks and special files. Require canonical root containment.
- Capture file identity and content revision at inspection, then canonicalize
  and revalidate both immediately before toolkit publication. An external edit,
  replacement, permission change, or path retarget produces a conflict.
- Preview validation/diffs before publication. Multi-file publication is one
  toolkit transaction; disk-full, permission, rename, crash, and validation
  faults publish all intended revisions or none.
- Keep bounded recovery journals outside the project. Recovery is explicit,
  visible, expiring, and never silently written over source.
- Never execute project scripts, shell fragments, plugins, macros, or build
  instructions merely by opening a project. Automation is an explicit bounded
  command with dry-run defaults.
- Never connect to, start, embed, stop, or mutate a game server. Playtest is a
  versioned wrapper request with isolated state and cleanup.

## Current M1 proof and residual work

`RelativePath`, `PathPolicy`, `FileStamp`, and `PathProbe` enforce lexical,
class, canonical, type, identity, and revision checks with deterministic fakes.
They intentionally perform no real filesystem operation. Issue #4 integrates
the released toolkit publisher and OS-specific canonical/file-identity adapter;
issue #9 owns fault, race, symlink-swap, permission, disk-full, and crash tests.
