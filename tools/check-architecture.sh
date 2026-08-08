#!/usr/bin/env bash
set -euo pipefail

metadata=$(mktemp /tmp/atrinik-editor-metadata.XXXXXX)
trap 'rm -f -- "${metadata}"' EXIT
cargo metadata --locked --offline --format-version 1 >"${metadata}"

jq -e '
  def deps($name): [.packages[] | select(.name == $name) | .dependencies[].name] | sort;
  deps("atrinik-editor-project") == [] and
  deps("atrinik-editor-document") == ["atrinik-source"] and
  deps("atrinik-editor-commands") == ["atrinik-source","atrinik-transaction"] and
  deps("atrinik-editor-ui") == ["atrinik-editor-project"] and
  deps("atrinik-editor-preview") == ["atrinik-render-api","atrinik-render-resources","atrinik-render-testkit","atrinik-scene"] and
  deps("atrinik-editor-testkit") == ["atrinik-editor-project"] and
  deps("atrinik-editor") == ["atrinik-editor-project","sdl3"] and
  ([.packages[].name | select(test("(client|protocol|server|classic|gridarta)"; "i"))] | length == 0) and
  all(.packages[].dependencies[];
    (.source // "") as $source |
    ($source == "" or
      $source == "registry+https://github.com/rust-lang/crates.io-index" or
      ($source | startswith("git+https://github.com/atrinik/content-toolkit?rev=b2178d442af5d897a45619c200fec5ceb39fc3cf")) or
      ($source | startswith("git+https://github.com/atrinik/renderer?rev=3a6bbeabc2b7eac8d162d758732a0495fe8a9dd9"))))
' "${metadata}" >/dev/null
