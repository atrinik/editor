# Editing engine

Every editing front end (the desktop app today; a command line and an MCP
server later, issue #57) drives one engine: `Session` in the
`tilewright-editor-ui` crate. The engine has no SDL, GPU, font, layout or
direct file access. A front end builds `Action`s, calls `Session::dispatch`,
and reads the result back; it adds no editing logic of its own and cannot
bypass anything the desktop app enforces. See
[ADR 0002](../decisions/0002-one-editing-engine.md).

## What the engine owns

- **Preparation.** Every edit is one World transaction, prepared against the
  revision its author observed, limited by the catalog's grants and active
  view, and fenced so a pending change cannot be replaced before Accept or
  Cancel.
- **Review.** After every dispatch the engine builds the review of the
  pending change (`Session::review`): notes and deletion warnings first,
  within their own 16 KiB allowance, then a change-centred excerpt of every
  changed file, within 64 KiB of escaped excerpt rows and at most 32 changed
  files. A review that does not fit is incomplete and starts with "Change
  exceeds the visible review limit. Cancel this edit."
- **The review gate.** `Action::Accept` through `Session::dispatch` is
  refused, with the code review.incomplete, while the review is incomplete. The
  pending change stays for Cancel. The desktop app checks the same flag
  first to disable its Accept button, but the engine does not rely on it.
- **The actor.** `Session::new` takes an `Actor`, the `actor` of every
  transaction the session prepares: non-empty, at most 256 characters (the
  transaction format's limit) and free of control characters, for example
  `editor:prototype` (the desktop app), `human:<name>` or `agent:<client>`.
  A redo replays the accepted edit with its actor. An undo replays World's
  inverse of it, which records no actor.
- **Summaries.** `Session::pending_summary` gives a bounded structured view
  of the pending change for callers that do not draw it: base and result
  revisions, at most 32 changed files with their change kind and sizes (and
  how many more were left out), the review lines, the notes and warnings
  the review shows (and how many it left out), and whether Accept may
  publish it. World text in it is unescaped data; a front end that prints
  or returns it escapes and labels it.

The front end owns everything else: layout and drawing, input, the frame
selection, the recovery store it passes to `dispatch` and the recovery
descriptor it reports back with `Session::set_recovery_descriptor`.

## Addressing

Every edit action names its target by uid, plane, layer, grid and plane
coordinates, never by the frame selection: `Move`, `SetField`, `Remove`
and `SetPortal` by uid; `Paint`, `PaintStroke`, `PaintZones` and `Place` by
plane tile; zone and anchor actions by plane and id. `Select` and
`ClearSelection` only change what the desktop app highlights; an accepted
placement selects the new object. Targets must still be in the catalog's
active view, because the catalog grants edits only there.

## Refusal codes

Every `SessionError` carries a stable `RefusalCode` (`code()`, and
`as_str()` for its text) beside its message (`message()`, also `Display`).
Messages are for people and may be reworded; codes are stable.

| Code | Meaning |
|---|---|
| `session.blocked` | The session is blocked after an uncertain publication; only Reopen is accepted. |
| `pending.exists` | A change is pending review: Accept or Cancel it first. |
| `pending.none` | Accept found no pending change. |
| `revision.stale` | The action names a source revision other than the open one. |
| `source.changed` | The source on disk differs from the World the session opened. |
| `source.invalid` | The source cannot be read, is not UTF-8, does not parse or load, or lacks a document the action needs. |
| `catalog.refused` | The editor catalog or its resources no longer verify. |
| `selection.stale` | A selection names a stale frame or an unknown semantic key. |
| `grant.missing` | The catalog does not grant this edit. |
| `scope.outside_view` | The target is outside the active view, or is not a placed object of it. |
| `target.unknown` | The named object, plane, zone, anchor, grid or field does not exist. |
| `target.exists` | A new id is already used. |
| `target.in_use` | The target is still referenced and cannot be deleted. |
| `target.unsupported` | The target exists but cannot take this edit, for example an object in an inventory. |
| `hierarchy.cycle` | A zone parent would make a cycle. |
| `input.identifier` | A new id does not have the World identifier shape. |
| `input.invalid` | The action's input is malformed, for example unordered stroke cells. |
| `input.budget` | The action's input exceeds a value, length or count budget. |
| `change.none` | The action would change nothing. |
| `actor.invalid` | The actor is empty, too long or contains a control character. |
| `world.refused` | World refused the prepared transaction; nothing is pending. |
| `review.incomplete` | The pending change's review is incomplete, so Accept is refused. |
| `history.full` | History has no room for another step. |
| `history.nothing_to_undo` | There is no step to undo. |
| `history.nothing_to_redo` | There is no step to redo. |
| `recovery.failed` | The recovery store did not retain the inverse; nothing was published. |
| `publish.failed` | Publication failed; the status reports what was applied. |
| `publish.unverified` | Publication was applied, but the reopened source differs or does not reopen. |
| `reopen.blocked` | Reopen found differing source or an uncertain publication. |
| `uid.unavailable` | No unused uid could be drawn for a new object. |
| `internal.unexpected` | An unexpected internal failure: a bug, not a property of the input. |

A test checks that every code is listed here exactly once.
