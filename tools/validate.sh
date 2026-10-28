#!/usr/bin/env bash
set -euo pipefail

repository=$(git rev-parse --show-toplevel)
cd "${repository}"
python3 -m unittest discover -s tools -p 'test_package_provenance.py'
python3 -m unittest discover -s tools -p 'test_check_windows_imports.py'
python3 -m unittest discover -s tools -p 'test_third_party_notices.py'
test "$(rustc --version | awk '{print $2}')" = 1.97.1
for command in cargo cargo-deny cargo-auditable jq syft; do command -v "${command}" >/dev/null || { echo "missing required tool: ${command}" >&2; exit 1; }; done
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked --workspace --doc
cargo deny --locked check
python3 tools/third-party-notices.py --check
tools/check-foundations.sh
cargo run --locked --quiet --package tilewright-editor -- version
cargo run --locked --quiet --package tilewright-editor -- open crates/tilewright-editor-testkit/fixtures/meadow
cargo run --locked --quiet --package tilewright-editor -- paint crates/tilewright-editor-testkit/fixtures/meadow --plane meadow --terrain sand --brush circle:1 30,3 33,3 >/dev/null
cargo run --locked --quiet --package tilewright-editor -- headless crates/tilewright-editor-testkit/fixtures/meadow
first=$(mktemp -d /tmp/tilewright-editor-release-first.XXXXXX); rmdir "${first}"
second=$(mktemp -d /tmp/tilewright-editor-release-second.XXXXXX); rmdir "${second}"
trap 'rm -rf -- "${first}" "${second}"' EXIT
tools/package-linux.sh "${first}" 0.1.0-test.1
tools/package-linux.sh "${second}" 0.1.0-test.1
cmp "${first}/tilewright-editor-0.1.0-test.1-linux-amd64.tar.gz" "${second}/tilewright-editor-0.1.0-test.1-linux-amd64.tar.gz"
cmp "${first}/tilewright-editor-0.1.0-test.1-source.tar.gz" "${second}/tilewright-editor-0.1.0-test.1-source.tar.gz"
git diff --check
