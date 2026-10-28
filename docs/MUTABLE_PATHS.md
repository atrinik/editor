# Mutable path ownership

| Path class | Owner | Mutation contract | Recovery/user visibility |
| --- | --- | --- | --- |
| Authored source roots | world toolkit transaction publisher | explicit semantic command that names the values it expects to replace, validation, dry-run diff, canonical revalidation, atomic publish; a session publishes each interaction when it ends and undoes it with the toolkit's inverse, whose file digests undo and redo check | dirty/conflict/diff/save state visible per document |
| Inverse records the user keeps (`paint --save INVERSE`) | the command line, through the toolkit's `write_new` | a new file only, never replacing one, and only outside the source root; a bind mount of a folder inside the tree is not recognised: an inverse written through one under `planes/` or `defs/` makes publishing refuse the stray file until it is removed, and one elsewhere in the tree stays there unnoticed, so keep `--save` outside the world | the user keeps it, or applies it with `tilewright tx apply` |
| Project/editor preferences | editor project service | bounded typed update, atomic editor-owned storage | reset/export visible; never mixed with authored source |
| Autosave/recovery | editor recovery service | append/replace outside project with quotas and expiry | recovery chooser; explicit discard/restore |
| Generated/collected/build output | world toolkit compiler | replace versioned output root, never treated as source | build result and provenance visible |
| Renderer cache | renderer resource provider | digest/revision keyed and bounded | safe eviction; diagnostics only |
| Game build/runtime/state/logs | the consuming game's workspace tooling | that game's profile/topology/state contracts only | the game's own status and log commands |
| Credentials/trust | owning OS/wrapper service | never an editor document or snapshot | never rendered, logged, recovered, or packaged |

No panel, tool, preview adapter, SDL callback, watcher, or application shell may
write a file directly. Editor crates change source files only through the
toolkit's transaction publisher, and write an inverse the user asked to keep
only through the toolkit's `write_new`.

A save expects the values it replaces, not a revision of the whole file: an
outside change to a tile the save paints is a conflict, and the session drops
that save and reads the world again, while an outside change elsewhere in the
same file is kept. Undo and redo expect the exact files their save left, so any
outside change to those files is a conflict for them. Each save is one
transaction; an undo step that coalesced several saves undoes and redoes them
one transaction at a time (`docs/THREAT_MODEL.md`).
