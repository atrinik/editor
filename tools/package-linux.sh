#!/usr/bin/env bash
set -euo pipefail

repository=$(git rev-parse --show-toplevel)
cd "${repository}"
output=${1:-dist}
version=${2:-0.1.0-dev}
font_pack=${3:?usage: package-linux.sh OUTPUT VERSION PREPARED_FONT_PACK}
if [[ ! ${version} =~ ^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?$ ]]; then
  echo "invalid release version: ${version}" >&2
  exit 2
fi
if [[ -e ${output} ]]; then
  echo "release output already exists: ${output}" >&2
  exit 1
fi
for command in cargo-auditable syft strip python3; do command -v "${command}" >/dev/null || { echo "missing required tool: ${command}" >&2; exit 1; }; done

stage=$(mktemp -d /tmp/tilewright-editor-stage.XXXXXX)
trap 'rm -rf -- "${stage}"' EXIT
cargo auditable build --locked --release --package tilewright-editor
target=$(cargo metadata --locked --offline --format-version 1 --no-deps | jq -r .target_directory)
install -d "${stage}/tilewright-editor-${version}/bin" "${output}"
install "${target}/release/tilewright-editor" "${stage}/tilewright-editor-${version}/bin/tilewright-editor"
strip "${stage}/tilewright-editor-${version}/bin/tilewright-editor"
cp LICENSE PROVENANCE.md THIRD_PARTY_NOTICES.md \
  "${stage}/tilewright-editor-${version}/"
python3 tools/check-prototype-assets.py
# Every sample file must be on the package allowlist. The staged sample is
# opened as a World below, which does not read the preview catalog or its
# art; this test covers those too.
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools -p 'test_package_assets.py' >/dev/null
python3 tools/package-assets.py --root . --stage "${stage}/tilewright-editor-${version}"
python3 tools/package-font-notice.py --root . --stage "${stage}/tilewright-editor-${version}" >/dev/null
python3 tools/prepare-font-pack.py stage --root . --font-pack "${font_pack}" \
  --stage "${stage}/tilewright-editor-${version}" >/dev/null
"${stage}/tilewright-editor-${version}/bin/tilewright-editor" version >/dev/null
# The packaged sample must open as a World with the packaged binary: a fixture
# file left out of policy/package-assets.json would ship a sample that does not
# load.
"${stage}/tilewright-editor-${version}/bin/tilewright-editor" open \
  "${stage}/tilewright-editor-${version}/samples/prototype-neutral" >/dev/null
# The read-only commands need no editing support: the packaged sample's catalog
# and art load, and lint finds no error in it.
"${stage}/tilewright-editor-${version}/bin/tilewright-editor" overview --json \
  "${stage}/tilewright-editor-${version}/samples/prototype-neutral" >/dev/null
"${stage}/tilewright-editor-${version}/bin/tilewright-editor" lint \
  "${stage}/tilewright-editor-${version}/samples/prototype-neutral" >/dev/null
SYFT_CHECK_FOR_APP_UPDATE=false syft \
  "${stage}/tilewright-editor-${version}/bin/tilewright-editor" \
  --source-name tilewright-editor --source-version "${version}" \
  --output "cyclonedx-json=${stage}/tilewright-editor-${version}/sbom.cdx.json"
sbom="${stage}/tilewright-editor-${version}/sbom.cdx.json"
normalized="${sbom}.normalized"
digest=$(printf 'tilewright-editor:linux-amd64:%s' "${version}" | sha256sum | cut -d' ' -f1)
serial="urn:uuid:${digest:0:8}-${digest:8:4}-8${digest:13:3}-8${digest:17:3}-${digest:20:12}"
jq --arg version "${version}" --arg serial "${serial}" '
  .serialNumber = $serial |
  .metadata.timestamp = "1970-01-01T00:00:00Z" |
  .metadata.component["bom-ref"] = ("tilewright-editor-linux-amd64@" + $version) |
  (.components[] | select(.type == "file") | .name) = "/tilewright-editor"
' "${sbom}" >"${normalized}"
mv "${normalized}" "${sbom}"
jq -e '(.components // []) | length >= 10' "${sbom}" >/dev/null
python3 tools/prepare-font-pack.py sbom --root . --stage "${stage}/tilewright-editor-${version}" \
  --sbom "${sbom}" >/dev/null
python3 tools/package-provenance.py --root . --version "${version}" \
  --output "${stage}/tilewright-editor-${version}/provenance.json"

archive="${output}/tilewright-editor-${version}-linux-amd64.tar.gz"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='UTC 1970-01-01' \
  --mode='u+rwX,go+rX,go-w' \
  -C "${stage}" -cf - "tilewright-editor-${version}" | gzip -n >"${archive}"
git archive --format=tar --prefix="tilewright-editor-${version}/" HEAD \
  | gzip -n >"${output}/tilewright-editor-${version}-source.tar.gz"
(cd "${output}" && sha256sum ./* >SHA256SUMS)
