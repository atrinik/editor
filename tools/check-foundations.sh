#!/usr/bin/env bash
set -euo pipefail

repository=$(git rev-parse --show-toplevel)
cd "${repository}"

jq -e '
  .schema_version == 1 and (.rows | length >= 55) and
  ([.rows[].id] | length == (unique | length)) and
  all(.rows[];
    .id != "" and .domain != "" and .entry_point != "" and .behavior != "" and
    .fixture != "" and .expected != "" and .failure != "" and
    (.formats | type == "array") and (.preservation | IN("byte","semantic","both")) and
    .owner != "" and .issue > 0 and .milestone != "" and .surface != "" and
    .acceptance != "" and .status == "required" and .provenance != "")
' migration/behavior-parity.json >/dev/null
jq -e '.schema_version == 1 and .grant_used == false and all(.records[]; .status == "excluded")' provenance/reuse.json >/dev/null
jq -e '.schema_version == 1 and .assets == []' provenance/assets.json >/dev/null
jq -e '.schema_version == 1 and .rust == "1.97.1" and (.direct_external | length == 3)' policy/dependencies.json >/dev/null

if grep -RhE '^[[:space:]]*uses:' .github/workflows 2>/dev/null \
  | grep -Ev '@[0-9a-f]{40}([[:space:]]|$)' >/dev/null; then
  echo "workflow action is not pinned to an immutable commit" >&2
  exit 1
fi
for required in CONTRIBUTING.md PROVENANCE.md SECURITY.md THIRD_PARTY_NOTICES.md \
  decisions/0001-editor-architecture.md docs/THREAT_MODEL.md docs/MUTABLE_PATHS.md; do
  test -s "${required}"
done
tools/check-architecture.sh
