# ADR 0002: one editing engine for every front end

Status: accepted.

Issue #57 adds a command line and an MCP server beside the desktop app.
Each must be unable to do anything the desktop app refuses. Until now the
review text and its completeness gate (64 KiB of excerpt, 32 changed files,
a separate 16 KiB allowance for warnings) lived in the app crate, so a
caller driving `Session` directly could accept a change whose review was
cut short. Refusals were free text, and every transaction's actor was the
fixed string `editor:prototype`.

Decision:

- `Session` in `tilewright-editor-ui` is the engine. It builds the review
  after every dispatch and refuses Accept itself while the review is
  incomplete. Front ends lay out the engine's review lines; they own no
  review text or limits.
- The engine stays in the existing ui crate rather than a new crate: it
  already holds `Session`, `Action` and every edit module, has no SDL, GPU,
  font or file access, and the move needs no new dependency.
  `tilewright-editor-document` (already a dependency of
  `tilewright-editor-commands` and already in the crate's architecture
  list) becomes a normal dependency instead of a test-only one, for its
  terminal escaping.
- The actor is a validated `Actor` given to `Session::new`. The desktop app
  passes `editor:prototype`, so its transactions are unchanged.
- Every refusal has a stable `RefusalCode`; messages stay as they were.
- `Session::pending_summary` is a bounded structured view for callers that
  do not draw the review. Serialising it is the front end's job.

[The engine guide](../docs/ENGINE.md) lists the codes and the boundary.
Change sets, proposals, read-only queries and the front ends themselves are
later work under #57.
