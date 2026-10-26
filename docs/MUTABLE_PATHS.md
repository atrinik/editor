# Mutable path ownership

| Path class | Owner | Mutation contract | Recovery/user visibility |
| --- | --- | --- | --- |
| Authored source roots | world toolkit transaction publisher | explicit semantic command, expected revision, validation, dry-run diff, canonical revalidation, atomic publish; a session publishes the edits of an interaction when it ends, as one transaction whose inverse undoes it | dirty/conflict/diff/save state visible per document |
| Project/editor preferences | editor project service | bounded typed update, atomic editor-owned storage | reset/export visible; never mixed with authored source |
| Autosave/recovery | editor recovery service | append/replace outside project with quotas and expiry | recovery chooser; explicit discard/restore |
| Generated/collected/build output | world toolkit compiler | replace versioned output root, never treated as source | build result and provenance visible |
| Renderer cache | renderer resource provider | digest/revision keyed and bounded | safe eviction; diagnostics only |
| Game build/runtime/state/logs | the consuming game's workspace tooling | that game's profile/topology/state contracts only | the game's own status and log commands |
| Credentials/trust | owning OS/wrapper service | never an editor document or snapshot | never rendered, logged, recovered, or packaged |

No panel, tool, preview adapter, SDL callback, watcher, or application shell may
write a file directly. The current M1 crates expose no filesystem writer.
