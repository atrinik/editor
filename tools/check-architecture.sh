#!/usr/bin/env bash
set -euo pipefail

metadata=$(mktemp /tmp/tilewright-editor-metadata.XXXXXX)
trap 'rm -f -- "${metadata}"' EXIT
cargo metadata --locked --offline --format-version 1 >"${metadata}"

jq -e '
  def deps($name): [.packages[] | select(.name == $name) | .dependencies[].name] | sort;
  deps("tilewright-editor-project") == [] and
  deps("tilewright-editor-document") == ["tilewright-editor-testkit","tilewright-source"] and
  deps("tilewright-editor-commands") == ["tempfile","tilewright-edit","tilewright-editor-document","tilewright-editor-testkit","tilewright-jsonc","tilewright-source"] and
  deps("tilewright-editor-ui") == ["egui","tilewright-editor-project"] and
  deps("tilewright-editor-preview") == ["sha2","tilewright-render-api","tilewright-render-resources","tilewright-render-testkit","tilewright-render-world","tilewright-scene"] and
  deps("tilewright-editor-testkit") == ["tilewright-editor-project"] and
  deps("tilewright-editor") == ["eframe","tempfile","tilewright-edit","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-preview","tilewright-editor-testkit","tilewright-editor-ui","tilewright-render-api","tilewright-render-testkit","tilewright-render-wgpu","tilewright-render-world","tilewright-source","winit"] and
  # No game client, server or protocol. The window platform crates
  # are named here one by one, so a new match still fails.
  ([.packages[].name | select(test("(client|protocol|server)"; "i"))
    | select(IN("jobserver", "orbclient", "smithay-client-toolkit", "wayland-client",
        "wayland-protocols", "wayland-protocols-plasma", "wayland-protocols-wlr",
        "x11rb-protocol") | not)] | length == 0) and
  all(.packages[].dependencies[];
    (.source // "") as $source |
    ($source == "" or
      $source == "registry+https://github.com/rust-lang/crates.io-index"))
' "${metadata}" >/dev/null

# Integration tests may set up file system faults themselves; editor source
# may not touch the file system.
if grep -RnE --include='*.rs' --exclude-dir=tests \
  'std::(fs|net|process::Command)|tokio::|fn[[:space:]]+(parse|serialize|write|save)[[:space:]]*\(' \
  crates >/dev/null; then
  echo "editor source duplicates a forbidden parser/writer or direct I/O authority" >&2
  exit 1
fi
