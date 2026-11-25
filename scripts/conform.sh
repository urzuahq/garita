#!/usr/bin/env bash
# usage: scripts/conform.sh [GARITA] [KIT]
# Runs the agent CLI kit's conformance runner against a garita binary (default: the debug build)
# in a throwaway repository with a valid config and garita's hooks installed. KIT is a checkout of
# agent-cli-kit at the commit in tests/contract/VERSION; without it the script clones one.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
contract="$root/rust/crates/garita-core/tests/contract"
garita="$(realpath "${1:-$root/rust/target/debug/garita}")"
kit="${2:+$(realpath "$2")}"

if [ ! -x "$garita" ]; then
  echo "conform.sh: no garita binary at $garita; build it with cargo build first" >&2
  exit 2
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

read -r _ commit < "$contract/VERSION"
if [ -z "$kit" ]; then
  kit="$work/agent-cli-kit"
  git init -q "$kit"
  git -C "$kit" fetch -q --depth 1 https://github.com/urzuahq/agent-cli-kit.git "$commit"
  git -C "$kit" checkout -q FETCH_HEAD
fi
head="$(git -C "$kit" rev-parse HEAD 2>/dev/null || true)"
if [ "$head" != "$commit" ]; then
  echo "conform.sh: $kit is at ${head:-no commit}, not the pinned $commit" >&2
  exit 2
fi

repo="$work/repo"
mkdir -p "$repo/.garita" "$work/bin"
ln -s "$garita" "$work/bin/garita"
export PATH="$work/bin:$PATH"
git -C "$repo" init -q -b main
printf '.garita/receipts/\n' > "$repo/.gitignore"
printf 'schema_version: 1\nchecks:\n  noop:\n    run: "true"\n    parser: exit-code\n    on: [commit, push, ci]\n' \
  > "$repo/.garita/config.yaml"
(cd "$repo" && garita hooks install > /dev/null)
if ! (cd "$repo" && garita doctor) | grep -q '"status":"pass"'; then
  echo "conform.sh: garita doctor does not pass in the fixture repository, so the runner would test a not-run" >&2
  exit 2
fi

cd "$repo"
python3 -I "$kit/scripts/conform.py" "$work/bin/garita" --schemas "$contract" --require-run
