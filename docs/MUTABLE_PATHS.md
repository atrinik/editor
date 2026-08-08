# Mutable path ownership

| Path class | Owner | Mutation contract | Recovery/user visibility |
| --- | --- | --- | --- |
| Authored source roots | content-toolkit transaction publisher | explicit semantic command, expected revision, validation, dry-run diff, canonical revalidation, atomic publish | dirty/conflict/diff/save state visible per document |
| Project/editor preferences | editor project service | bounded typed update, atomic editor-owned storage | reset/export visible; never mixed with authored source |
| Autosave/recovery | editor recovery service | append/replace outside project with quotas and expiry | recovery chooser; explicit discard/restore |
| Generated/collected/build output | content-toolkit compiler/collector | replace versioned output root, never treated as source | build result and provenance visible |
| Renderer cache | renderer resource provider | digest/revision keyed and bounded | safe eviction; diagnostics only |
| Wrapper build/runtime/state/logs | `atrinik/atrinik` | wrapper profile/topology/state contracts only | wrapper `show`/`ps`/`logs`/`down` |
| Credentials/trust | owning OS/wrapper service | never an editor document or snapshot | never rendered, logged, recovered, or packaged |

No panel, tool, preview adapter, SDL callback, watcher, or application shell may
write a file directly. The current M1 crates expose no filesystem writer.
