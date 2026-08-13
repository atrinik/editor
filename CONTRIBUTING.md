# Contributing

Use MIT code and synthetic or separately licensed, manifested fixtures.
Independent implementation is the default where exact reuse is not proven.
Exact historical Classic material may be inspected or reused only when the
[local provenance record](PROVENANCE.md) and
[canonical grant registry](https://github.com/atrinik/atrinik/blob/main/docs/PROVENANCE.md)
admit every copyrightable portion at an exact source revision. Complete
rename-aware history must prove each portion is original past work solely
authored by its grantor; present-day blame, majority authorship, later edits,
and agent-assisted commits are insufficient. An admitted destination may copy,
adapt, port, translate, and MIT-relicense that material, but must not depend on
the GPL Classic source. The destination grant does not change the Classic
repository's GPL distribution. Record the exact evidence and exclude every
uncovered portion. Gridarta remains excluded without separately sufficient
evidence, and behavior observation alone does not authorize source reuse.

Direct human-written code contributions are welcome. They follow the same
evidence-gated provenance, maintainer review, tests, and repository validation
as code developed through the project's primary Codex-driven workflows; using
an agent is not a contribution requirement.

Keep authority in the owning released dependency. New editor code must not add
a parser, writer, renderer, server/client/protocol edge, direct filesystem write
from UI code, or manifest-local sibling override. Run `tools/validate.sh` and
use a Conventional Commit message and PR title.
