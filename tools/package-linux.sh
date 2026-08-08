#!/usr/bin/env bash
set -euo pipefail

repository=$(git rev-parse --show-toplevel)
cd "${repository}"
output=${1:-dist}
version=${2:-0.1.0-dev}
if [[ ! ${version} =~ ^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?$ ]]; then
  echo "invalid release version: ${version}" >&2
  exit 2
fi
if [[ -e ${output} ]]; then
  echo "release output already exists: ${output}" >&2
  exit 1
fi
for command in cargo-auditable syft strip; do command -v "${command}" >/dev/null || { echo "missing required tool: ${command}" >&2; exit 1; }; done

stage=$(mktemp -d /tmp/atrinik-editor-stage.XXXXXX)
trap 'rm -rf -- "${stage}"' EXIT
cargo auditable build --locked --release --package atrinik-editor
target=$(cargo metadata --locked --offline --format-version 1 --no-deps | jq -r .target_directory)
install -d "${stage}/atrinik-editor-${version}/bin" "${output}"
install "${target}/release/atrinik-editor" "${stage}/atrinik-editor-${version}/bin/atrinik-editor"
strip "${stage}/atrinik-editor-${version}/bin/atrinik-editor"
cp LICENSE PROVENANCE.md THIRD_PARTY_NOTICES.md policy/dependencies.json \
  "${stage}/atrinik-editor-${version}/"
"${stage}/atrinik-editor-${version}/bin/atrinik-editor" version >/dev/null
SYFT_CHECK_FOR_APP_UPDATE=false syft \
  "${stage}/atrinik-editor-${version}/bin/atrinik-editor" \
  --source-name atrinik-editor --source-version "${version}" \
  --output "cyclonedx-json=${stage}/atrinik-editor-${version}/sbom.cdx.json"
jq -e '(.components // []) | length >= 10' "${stage}/atrinik-editor-${version}/sbom.cdx.json" >/dev/null
jq -n --arg version "${version}" --arg revision "$(git rev-parse HEAD)" \
  --arg rust "$(rustc --version)" \
  '{schema_version:1,version:$version,revision:$revision,rust:$rust,
    toolkit:{release:"v1.0.0",revision:"b2178d442af5d897a45619c200fec5ceb39fc3cf"},
    renderer:{release:"v1.0.0",revision:"3a6bbeabc2b7eac8d162d758732a0495fe8a9dd9"}}' \
  >"${stage}/atrinik-editor-${version}/provenance.json"

archive="${output}/atrinik-editor-${version}-linux-amd64.tar.gz"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='UTC 1970-01-01' \
  -C "${stage}" -cf - "atrinik-editor-${version}" | gzip -n >"${archive}"
git archive --format=tar --prefix="atrinik-editor-${version}/" HEAD \
  | gzip -n >"${output}/atrinik-editor-${version}-source.tar.gz"
(cd "${output}" && sha256sum ./* >SHA256SUMS)
