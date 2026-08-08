#!/usr/bin/env bash
set -euo pipefail

repository=$(git rev-parse --show-toplevel)
cd "${repository}"
test "$(rustc --version | awk '{print $2}')" = 1.97.1
for command in cargo cargo-deny cargo-auditable jq syft; do command -v "${command}" >/dev/null || { echo "missing required tool: ${command}" >&2; exit 1; }; done
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked --workspace --doc
cargo deny --locked check
tools/check-foundations.sh
cargo run --locked --quiet --package atrinik-editor -- version
cargo run --locked --quiet --package atrinik-editor -- headless
SDL_VIDEO_DRIVER=dummy cargo run --locked --quiet --package atrinik-editor -- window
first=$(mktemp -d /tmp/atrinik-editor-release-first.XXXXXX); rmdir "${first}"
second=$(mktemp -d /tmp/atrinik-editor-release-second.XXXXXX); rmdir "${second}"
trap 'rm -rf -- "${first}" "${second}"' EXIT
tools/package-linux.sh "${first}" 0.1.0-test.1
tools/package-linux.sh "${second}" 0.1.0-test.1
cmp "${first}/atrinik-editor-0.1.0-test.1-linux-amd64.tar.gz" "${second}/atrinik-editor-0.1.0-test.1-linux-amd64.tar.gz"
cmp "${first}/atrinik-editor-0.1.0-test.1-source.tar.gz" "${second}/atrinik-editor-0.1.0-test.1-source.tar.gz"
git diff --check
