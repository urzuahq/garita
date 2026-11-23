#!/usr/bin/env bash
# Decides whether knope's `releases::no_release` exit is legitimate.
#
# It is, with no changeset fragments waiting: the ordinary case on a push that
# follows a release merge. It is not when fragments exist and none applied: a
# fragment naming a package `knope.toml` does not declare is silently inert.
#
# usage: release-guard.sh [changeset-dir] [knope-output]
# exit:  0 a legitimate no-release; 1 fragments present and inert; 2 the output
#        is not a no-release, so this guard has no verdict and the caller must
#        not read that as success.
set -uo pipefail

changeset_dir="${1:-.changeset}"
knope_output="${2:-}"

if ! printf '%s' "$knope_output" | grep -q 'releases::no_release'; then
  exit 2
fi

pending=$(find "$changeset_dir" -maxdepth 1 -name '*.md' 2>/dev/null | wc -l | tr -d ' ')
if [ "$pending" -gt 0 ]; then
  echo "::error::$pending changeset fragment(s) are present but none applied -- check the package key against knope.toml's [package]"
  exit 1
fi

echo "::notice::nothing to release -- no changeset fragment waiting"
exit 0
