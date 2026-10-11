# Released library updates

The editor's seven workspace packages are application implementation details and
have `publish = false`. Semantic-release and the Linux/Windows package workflows
continue to publish the editor application on GitHub.

`policy/released-dependencies.json` owns the six upstream library identities.
`tools/released_dependencies.py ready --root .` checks the manifest and lockfile
before compiler or aggregate validation begins. The explicitly recorded immutable
Git revisions are a temporary bootstrap allowance. Once crates.io publication is
verified, the updater replaces those dependencies with exact registry versions
and regenerates the lockfile through Cargo. A GitHub source archive alone cannot
satisfy registry readiness; no lockfile checksum is invented.

The hourly `Released dependency updates` workflow can also be dispatched manually
from `main`.
Its preparation job has no App credentials. A host-only read stage collects
bounded public release metadata with the read-only workflow token. The cache
contains allowlisted public fields and is sealed before being mounted read-only
into Cargo stages; read credentials are absent from those stages. It verifies the upstream versioned
release manifest, source commit, registry API and index, non-yanked version, and
the downloaded crate checksum, then exercises Cargo from an external consumer.
It regenerates only the owned dependency manifest and Cargo lockfile and runs the
editor aggregate checks in the pinned build image with two CPUs and isolated
caches. Preparation resolves without executing dependency code. A fresh trusted
checkout independently regenerates and compares the candidate lockfile. External
consumer and aggregate application checks then run with source mounted read-only,
no candidate artifacts mounted, and separately mounted caches. Candidate artifact
hashes are checked again afterward. Preparation failure never reaches publication.

A separate publication job receives `CRATE_DEPENDENCY_UPDATE_APP_PRIVATE_KEY`
(repository secret) and `CRATE_DEPENDENCY_UPDATE_APP_ID` (repository variable).
The dedicated `atrinik-crate-dependency-update` App is installed only on client
and editor with Contents and Pull requests write and Metadata read permissions.
Its token is absent from Cargo, dependency code, tests, and package builds. The
publisher checks the repository, candidate base and exact bot-owned branch/PR
identity and current branch head before publication through the Git Data API. It
reconstructs the permitted manifest edits from trusted policy and rejects
unexpected lockfile changes. It preserves human branches and human edits on bot
branches, detects duplicate update pull requests, and updates references without
forcing them. Updates require normal
review and validation; the workflow does not merge pull requests.

Keep this policy synchronized when adding or removing a released dependency.
Run the fake-registry helper tests, ShellCheck, actionlint and `tools/validate.sh`
for updater changes. Production registry verification and aggregate application
validation remain separate from fixture-test success.

Linux and Windows package provenance uses schema version 2. It records the
editor revision, compiler version, complete Cargo.lock digest, and the actual
locked version and source for each managed upstream library. Git bootstrap
dependencies include their verified immutable locked revision. Registry
dependencies include the locked archive checksum; a Git source revision or
repository release tag is omitted because Cargo.lock does not establish it.
The shared generator rejects mismatched manifest/lock coordinates and missing
registry checksums. Package metadata therefore changes with a dependency update
without a separate mutable provenance file or a guessed upstream release tag.
