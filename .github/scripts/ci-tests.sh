#!/usr/bin/env bash
# The one definition of how CI runs the Rust tests, shared by the checks
# workflow and `make test` so the two cannot drift. Every mode runs under
# test-no-leak.sh (a fresh TMPDIR that must be empty of garita-* entries
# afterwards).
#
#   ci-tests.sh nextest    the test suite under cargo-nextest
#   ci-tests.sh doctest    the doctests (nextest does not run them)
#   ci-tests.sh all        both (the default)
#
# Without cargo-nextest, `all` falls back to plain `cargo test` for a laptop,
# and says so; CI (CI=true) never falls back, so a missing tool cannot silently
# change what ran.
set -uo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
here="$root/.github/scripts"
cd "$root/rust" || exit 2

mode="${1:-all}"

noleak() { "$here/test-no-leak.sh" "$@"; }

have_nextest() { cargo nextest --version >/dev/null 2>&1; }

run_nextest() { noleak cargo nextest run --workspace --locked --no-fail-fast; }

run_doctest() { noleak cargo test --doc --workspace --locked; }

case "$mode" in
  nextest)
    have_nextest || { echo "ci-tests: cargo-nextest is not installed" >&2; exit 2; }
    run_nextest ;;
  doctest) run_doctest ;;
  all)
    if have_nextest; then
      run_nextest || exit $?
      run_doctest
    elif [ -n "${CI:-}" ]; then
      echo "ci-tests: cargo-nextest is not installed and CI is set" >&2; exit 2
    else
      echo "ci-tests: cargo-nextest not installed; running plain cargo test (CI uses nextest)" >&2
      noleak cargo test --workspace --locked --no-fail-fast
    fi ;;
  *) echo "usage: ci-tests.sh nextest|doctest|all" >&2; exit 2 ;;
esac
