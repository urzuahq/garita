#!/usr/bin/env bash
set -u

here=$(cd "$(dirname "$0")" && pwd -P)
ws=$here/workspace
out=$here/recordings
scratch=$(mktemp -d)

export PATH=$ws/node_modules/.bin:$PATH
export NO_COLOR=1 TURBO_TELEMETRY_DISABLED=1 DO_NOT_TRACK=1 TURBO_CACHE_DIR=.turbo/cache
unset CI GITHUB_ACTIONS FORCE_COLOR TURBO_UI
unset AI_AGENT AGENT CLAUDECODE CURSOR_AGENT CURSOR_TRACE_ID GEMINI_CLI CODEX_SANDBOX OPENCODE

pkgver() { node -p "require('$ws/node_modules/$1/package.json').version"; }
oxlint_dir=oxlint@$(pkgver oxlint)
oxfmt_dir=oxfmt@$(pkgver oxfmt)
tsc_dir=tsc@$(pkgver typescript)
tsgo_dir=tsgo@$(pkgver @typescript/native-preview)
turbo_dir=turbo@$(pkgver turbo)

clean_variants() {
  local f
  for f in "$ws"/variants/*; do
    rm -f "$ws"/packages/*/src/"$(basename "$f")"
  done
}

clean_turbo() {
  rm -rf "$ws"/.turbo "$ws"/packages/*/.turbo
}

reset_build() {
  rm -rf "$ws"/packages/*/dist "$ws"/packages/*/*.tsbuildinfo
  (cd "$ws" && tsc -b packages/lib) >/dev/null 2>&1
}

cleanup() {
  clean_variants
  clean_turbo
  rm -rf "$ws"/packages/*/dist "$ws"/packages/*/*.tsbuildinfo "$scratch"
}
trap cleanup EXIT

swap() {
  cp "$ws/variants/$2" "$ws/packages/$1/src/$2"
}

pnpm_bin=$(dirname "$(command -v pnpm)")

scrub() {
  local f
  for f in "$@"; do
    P=$pnpm_bin perl -pi -e 's/\Q$ENV{P}\E/<PNPM_BIN>/g' "$f"
    WS=$ws perl -pi -e 's/\Q$ENV{WS}\E/<WORKSPACE>/g' "$f"
    perl -pi -e 's/("sha": ")[0-9a-f]{40}(")/$1<SHA>$2/; s/("branch": ")[^"]*(")/$1<BRANCH>$2/' "$f"
    if [ -n "${ROOT_SCRUB:-}" ]; then
      R=$ROOT_SCRUB perl -pi -e 's/\Q$ENV{R}\E/<WORKSPACE>/g' "$f"
    fi
  done
}

ROOT=$ws
ROOT_SCRUB=

rec() {
  local dir=$1 name=$2 cwd=$3
  shift 3
  local d=$out/$dir/$name
  mkdir -p "$d"
  { echo "$cwd"; printf '%s\n' "$@"; } >"$d/cmd"
  (cd "$ROOT/$cwd" && "$@") >"$d/stdout" 2>"$d/stderr"
  echo $? >"$d/exit_code"
  scrub "$d/cmd" "$d/stdout" "$d/stderr"
}

rm -rf "$out"
clean_variants
clean_turbo
reset_build

lint_both() {
  local name=$1 cwd=$2
  shift 2
  rec "$oxlint_dir" "$name-json" "$cwd" oxlint --format json "$@"
  rec "$oxlint_dir" "$name-default" "$cwd" oxlint "$@"
}

lint_both clean . packages/app/src
swap app lint-error.ts
lint_both one-error . packages/app/src
lint_both one-error-in-package packages/app src
rec "$oxlint_dir" one-error-default-agent-env . env CLAUDECODE=1 oxlint packages/app/src
rec "$oxlint_dir" one-error-json-agent-env . env CLAUDECODE=1 oxlint --format json packages/app/src
swap app lint-error-2.ts
lint_both errors-two-files . packages/app/src
clean_variants
swap app lint-warn.ts
lint_both warning-only . packages/app/src
clean_variants
lint_both no-files-matched . 'packages/app/nothing/**/*.ts'
lint_both no-files-matched-allowed . --no-error-on-unmatched-pattern 'packages/app/nothing/**/*.ts'
lint_both nonexistent-path . packages/app/missing
lint_both invalid-config . -c variants/bad-oxlintrc.json packages/app/src
rec "$oxlint_dir" version . oxlint --version

fmt_both() {
  local name=$1 cwd=$2 mode
  shift 2
  for mode in --check --list-different; do
    rec "$oxfmt_dir" "${mode#--}-$name" "$cwd" oxfmt "$mode" "$@"
  done
}

fmt_both formatted . packages/app/src
fmt_both formatted-in-package packages/app src
swap app format-bad.ts
fmt_both one-unformatted . packages/app/src
fmt_both one-unformatted-in-package packages/app src
swap app format-bad-2.ts
fmt_both two-unformatted . packages/app/src
clean_variants
fmt_both no-files-matched . 'packages/app/nothing/**/*.ts'
fmt_both no-files-matched-allowed . --no-error-on-unmatched-pattern 'packages/app/nothing/**/*.ts'
fmt_both nonexistent-path . packages/app/missing
swap app syntax-error.ts
fmt_both syntax-error . packages/app/src
clean_variants
swap app unknown.xyz
fmt_both unknown-file-type-explicit . packages/app/src/unknown.xyz
fmt_both unknown-file-type-in-dir . packages/app/src
clean_variants
rec "$oxfmt_dir" version . oxfmt --version

ts_rec() {
  local tool=$1 name=$2 cwd=$3
  shift 3
  [ -n "${KEEP_BUILD:-}" ] || reset_build
  rec "$tool@$(pkgver "$([ "$tool" = tsc ] && echo typescript || echo @typescript/native-preview)")" "$name" "$cwd" "$tool" "$@"
}

ts_cases() {
  local t=$1
  ts_rec "$t" clean . --noEmit --pretty false -p packages/app
  ts_rec "$t" clean-in-package packages/app --noEmit --pretty false
  swap app type-error.ts
  ts_rec "$t" type-error . --noEmit --pretty false -p packages/app
  ts_rec "$t" type-error-in-package packages/app --noEmit --pretty false
  reset_build
  (cd "$ws" && "$t" --noEmit --pretty false -p packages/app) >/dev/null 2>&1
  KEEP_BUILD=1 ts_rec "$t" type-error-second-run . --noEmit --pretty false -p packages/app
  swap app type-error-2.ts
  ts_rec "$t" errors-two-files . --noEmit --pretty false -p packages/app
  ts_rec "$t" solution-build-with-errors . -b --pretty false
  clean_variants
  swap app type-error-multiline.ts
  ts_rec "$t" type-error-multiline . --noEmit --pretty false -p packages/app
  clean_variants
  swap app syntax-error.ts
  ts_rec "$t" syntax-error . --noEmit --pretty false -p packages/app
  clean_variants
  ts_rec "$t" solution-plain . --noEmit --pretty false -p .
  ts_rec "$t" solution-build . -b --pretty false
  ts_rec "$t" nonexistent-project . --noEmit --pretty false -p packages/missing
  ts_rec "$t" list-files-only . --noEmit --pretty false --listFilesOnly -p packages/app
  ts_rec "$t" list-files . --noEmit --pretty false --listFiles -p packages/app
  ts_rec "$t" extended-diagnostics . --noEmit --pretty false --extendedDiagnostics -p packages/app
  ts_rec "$t" version . --version
}

ts_cases tsc
ts_cases tsgo

turbo_collect() {
  local d=$1 f rel
  local run
  run=$(ls -t "$ROOT"/.turbo/runs/*.json 2>/dev/null | head -n1)
  if [ -n "$run" ]; then
    cp "$run" "$d/summary.json"
    scrub "$d/summary.json"
  fi
  for f in $(cd "$ROOT" && find packages -path '*/.turbo/*.log' | sort); do
    rel=$(dirname "$(dirname "$f")")
    mkdir -p "$d/logs/$rel"
    cp "$ROOT/$f" "$d/logs/$rel/$(basename "$f")"
    scrub "$d/logs/$rel/$(basename "$f")"
  done
}

turbo_rec() {
  local name=$1
  shift
  rec "$turbo_dir" "$name" . turbo "$@"
  turbo_collect "$out/$turbo_dir/$name"
}

turbo_fresh() {
  clean_turbo
  reset_build
  rm -rf "$ROOT"/.turbo "$ROOT"/packages/*/.turbo
}

turbo_modes() {
  local task=$1 prefix=$2
  local m
  turbo_fresh
  turbo_rec "$prefix-default" run "$task"
  turbo_fresh
  turbo_rec "$prefix-errors-only" run "$task" --output-logs=errors-only
  turbo_fresh
  turbo_rec "$prefix-log-order-grouped" run "$task" --log-order=grouped
  turbo_fresh
  turbo_rec "$prefix-log-order-stream" run "$task" --log-order=stream
  turbo_fresh
  turbo_rec "$prefix-log-prefix-none" run "$task" --log-prefix=none
  turbo_fresh
  turbo_rec "$prefix-continue" run "$task" --continue
  turbo_fresh
  turbo_rec "$prefix-summarize" run "$task" --summarize
  turbo_fresh
  turbo_rec "$prefix-dry-json" run "$task" --dry=json
}

swap app type-error.ts
turbo_modes type-check type-check
clean_variants

swap lib lint-error.ts
swap docs lint-warn.ts
turbo_modes lint:check lint-check
clean_variants

turbo_fresh
turbo_rec type-check-all-passing run type-check
turbo_rec type-check-all-cached run type-check
turbo_fresh
turbo_rec lint-check-all-passing run lint:check
turbo_rec lint-check-all-cached run lint:check
turbo_fresh
turbo_rec type-check-and-lint-check-all-passing run type-check lint:check
swap app type-error.ts
swap lib lint-error.ts
turbo_fresh
turbo_rec type-check-and-lint-check-two-failing run type-check lint:check
turbo_fresh
turbo_rec type-check-and-lint-check-two-failing-continue run type-check lint:check --continue
clean_variants

turbo_fresh
turbo_rec version --version

turbo_affected() {
  local name=$1 gitinit=$2
  local copy=$scratch/$name
  mkdir -p "$copy"
  (cd "$ws" && tar --exclude=.turbo --exclude=dist --exclude='*.tsbuildinfo' -cf - .) | (cd "$copy" && tar -xf -)
  if [ "$gitinit" = yes ]; then
    (
      cd "$copy" &&
        export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null &&
        git init -q -b main &&
        git add -A &&
        git -c user.name=fixture -c user.email=fixture@example.invalid commit -q -m fixture
    )
  fi
  ROOT=$copy
  ROOT_SCRUB=$copy
  rec "$turbo_dir" "$name" . turbo run type-check --affected
  turbo_collect "$out/$turbo_dir/$name"
  ROOT=$ws
  ROOT_SCRUB=
}

turbo_affected affected-no-git no
turbo_affected affected-git-unchanged yes
