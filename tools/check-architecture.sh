#!/usr/bin/env bash
set -euo pipefail

metadata=$(mktemp /tmp/tilewright-editor-metadata.XXXXXX)
trap 'rm -f -- "${metadata}"' EXIT
cargo metadata --locked --offline --format-version 1 >"${metadata}"

jq -e '
  def deps($name): [.packages[] | select(.name == $name) | .dependencies[].name] | sort;
  deps("tilewright-editor-project") == [] and
  deps("tilewright-editor-document") == ["tilewright-editor-testkit","tilewright-source"] and
  deps("tilewright-editor-commands") == ["tempfile","tilewright-edit","tilewright-editor-document","tilewright-editor-testkit","tilewright-source"] and
  deps("tilewright-editor-ui") == ["serde_json","tempfile","tilewright-edit","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-preview","tilewright-editor-project","tilewright-editor-testkit","tilewright-jsonc","tilewright-source","unicode-segmentation"] and
  deps("tilewright-editor-preview") == ["serde_json","sha2","tempfile","tilewright-jsonc","tilewright-render-api","tilewright-render-resources","tilewright-render-testkit","tilewright-render-world","tilewright-scene","tilewright-source"] and
  deps("tilewright-editor-testkit") == ["serde_json","sha2","tilewright-editor-project","tilewright-source"] and
  deps("tilewright-editor-recovery") == ["sha2","tempfile","tilewright-edit","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-testkit","tilewright-source"] and
  deps("tilewright-editor") == ["png","pollster","sdl3","serde_json","sha2","tempfile","tilewright-edit","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-preview","tilewright-editor-recovery","tilewright-editor-testkit","tilewright-editor-ui","tilewright-render-api","tilewright-render-resources","tilewright-render-sdl3","tilewright-render-testkit","tilewright-render-ui","tilewright-render-wgpu","tilewright-render-world","tilewright-scene","tilewright-source"] and
  ([.packages[].name | select(test("(client|protocol|server)"; "i"))] | length == 0) and
  all(.packages[].dependencies[];
    (.source // "") as $source |
    ($source == "" or
      $source == "registry+https://github.com/rust-lang/crates.io-index"))
' "${metadata}" >/dev/null

# Integration tests alone may create disposable source trees and inject faults.
# Production crates (including the testkit library) retain the no-I/O contract.
if grep -RnE --include='*.rs' --exclude-dir=tests --exclude=tests.rs \
  'std::(fs|net|process::Command)|tokio::|fn[[:space:]]+(parse|serialize|write|save)[[:space:]]*\(' \
  crates >/dev/null; then
  echo "editor source duplicates a forbidden parser/writer or direct I/O authority" >&2
  exit 1
fi
