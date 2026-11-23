#!/usr/bin/env bash
# Two facts about a release that nothing else checks.
#
# usage: release-invariants.sh [changeset-dir] [manifest] [newest-tag]
#   newest-tag is empty when the repository has no tags yet.
set -uo pipefail

changeset_dir="${1:-.changeset}"
manifest="${2:-rust/Cargo.toml}"
latest_tag="${3:-}"

fail=0

# 1. A release-prep version bump must not reach main ahead of its tag.
#
# With fragments pending, nothing has been released since the last tag, so the
# manifest must still carry that tag's version. Only checked while fragments
# are pending: the release workflow merges before it tags, so a legitimate
# window exists where the manifest leads the tag, and that window always has
# an empty changeset directory.
#
# With no tag at all nothing has been released, so there is nothing to compare;
# a shallow checkout also has no tags, and that is refused rather than read as
# "no releases yet".
pending=$(find "$changeset_dir" -maxdepth 1 -name '*.md' 2>/dev/null | wc -l | tr -d ' ')
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$manifest" | head -1)

if [ "$pending" -gt 0 ]; then
  if [ -z "$latest_tag" ]; then
    if [ "$(git rev-parse --is-shallow-repository 2>/dev/null)" = "true" ]; then
      echo "::error::shallow checkout: the tags cannot be read, so $manifest's version cannot be compared against the newest tag"
      fail=1
    fi
  elif [ "v$version" != "$latest_tag" ]; then
    echo "::error::$manifest says $version but the newest tag is $latest_tag, with $pending changeset fragment(s) still pending -- a release-prep bump has reached this branch ahead of its release"
    fail=1
  fi
fi

# 2. A fragment describing a breaking change must declare `major`.
#
# Pre-1.0, knope maps major to a minor bump and minor to a patch bump, so
# `minor` on a breaking change ships it as a patch with no Breaking Changes
# section in the changelog.
for fragment in "$changeset_dir"/*.md; do
  [ -e "$fragment" ] || continue
  level=$(awk '
    NR == 1 { if ($0 != "---") exit; next }
    $0 == "---" { exit }
    /^default:[ \t]*/ { sub(/^default:[ \t]*/, ""); sub(/[ \t]+$/, ""); print; exit }
  ' "$fragment")
  [ -n "$level" ] || continue
  [ "$level" = "major" ] && continue
  # A declaration (`Breaking:` at the start of a line), not a mention in prose.
  if grep -qE '^\*{0,2}Breaking[:*]' "$fragment"; then
    echo "::error::$fragment declares '$level' but its text describes a breaking change -- pre-1.0 that ships as a $( [ "$level" = "minor" ] && echo patch || echo "$level" ) release with no Breaking Changes section"
    fail=1
  fi
done

[ "$fail" -eq 0 ] && echo "::notice::release invariants hold: manifest $version, newest tag ${latest_tag:-none}, $pending fragment(s) pending"
exit "$fail"
