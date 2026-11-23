#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dir="$root/rust/crates/garita-core/tests/contract"
cd "$dir"

release=0
[ "${1:-}" = "--release" ] && release=1

version=0.39.0
if command -v uvx >/dev/null 2>&1; then
  run=(uvx "check-jsonschema==$version")
elif command -v pipx >/dev/null 2>&1; then
  run=(pipx run --spec "check-jsonschema==$version" check-jsonschema)
else
  echo "contract-check.sh: neither uvx nor pipx is installed" >&2
  exit 2
fi

# The schemas' $id is a name, not a location; resolve $refs against the vendored copy.
base_uri="file://$dir/"

shopt -s nullglob
examples=(examples/*.json)
if [ ${#examples[@]} -eq 0 ]; then
  echo "contract-check.sh: no examples found" >&2
  exit 1
fi

failed=0

if ! sha256sum -- *.schema.json | diff - SHA256SUMS >&2; then
  echo "FAIL  SHA256SUMS does not match the vendored schemas" >&2
  failed=1
fi

for f in "${examples[@]}"; do
  name="$(basename "$f")"
  schema="${name%%.*}"
  if [ ! -f "$schema.schema.json" ]; then
    echo "FAIL  $f names no schema $schema.schema.json" >&2
    failed=1
  elif out="$("${run[@]}" --base-uri "$base_uri" --schemafile "$schema.schema.json" "$f" 2>&1)"; then
    echo "ok    $f"
  else
    echo "FAIL  $f should validate against $schema" >&2
    echo "$out" >&2
    failed=1
  fi
done

if [ "$release" -eq 1 ]; then
  read -r _ commit < VERSION
  if ! curl -fsSL "https://raw.githubusercontent.com/urzuahq/agent-cli-kit/$commit/schemas/SHA256SUMS" \
    | diff - SHA256SUMS >&2; then
    echo "FAIL  SHA256SUMS does not match the release in VERSION" >&2
    failed=1
  fi
fi

if [ "$failed" -ne 0 ]; then
  exit 1
fi
echo "${#examples[@]} examples validate"
