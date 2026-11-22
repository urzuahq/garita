---
Stable-Id: 01M7XKPYXGR0XNEG2CTRGY6WRG
Status: Draft
Date: 2026-11-21
Author: beauwilliams
---
# 1 — Garita: a machine-native gate for agent-authored change

## Summary

Garita runs the checks a change must pass (lint, format, types, tests, record checks, anything with an
exit code) and returns one verdict that an agent, a git hook and CI all read the same way. It is
declared once, in `.garita/config.yaml`, and run from four surfaces: commit, push, CI and merge.
Its output is the urzua contract: one JSON object on stdout with a `status`, typed `findings[]` and a
`next_action`, the one command to run next.

Garita judges the change. That is the line between it and urzua, which checks the records a change
traces to and never the change itself. The two compose through the shared contract and through
**receipts**: each garita run writes a self-dating, self-scoping record of what ran, against which
commit, and how long it took, which urzua can cite as evidence and decision-agent-kit can read as fact.

## Motivation

**Agents run checks badly, and every repository writes its own gate.** A gate today is a shell hook,
a `make` target and a CI workflow that each list the checks again and drift apart. Agents meet it as
text: a wall of linter output, or a hook that fails with no hint of what to run next. They re-run the
whole suite to check one fix, wait on slow steps they did not need, or route around the hook.

The evidence, from three weeks of agent sessions across this author's machines and repositories:

- Checks dominate an agent's waiting time. Full builds, type checks and test suites were run again after
  every small edit, where the changed package or the failing test would have answered.
- A TypeScript monorepo's commit hook ran the whole tree's lint and format on every commit; scoping it
  to staged files took it from tens of seconds to under one.
- A hook that a fresh worktree did not install, and a hook that called a package manager the machine
  did not have, both failed silently or confusingly. An agent can't tell a gate that passed from a
  gate that did not run.

**The output is the problem, not the runner.** Hook managers run commands well and in parallel. None
returns a verdict an agent can branch on. An agent-gate tool trialled on 2026-11-21 had the right
idea (one config, an agent hook that blocks the commit, a versioned JSON verdict) and showed exactly
where it falls short:

| Observed | Why it matters |
|---|---|
| A failing lint check reported `check exited with status 1`, with no file, line or message | The agent gets less than the raw output it would have read without the gate |
| A type check pointed at a solution `tsconfig` with no files passed with no output | A check that ran on nothing is indistinguishable from a pass |
| Run without its environment variable, the gate printed `skipping` and exited 0 | A broken install reads as green |
| Adopting the repository's existing hooks reduced each to its first line, `set -e` | Migration produced a gate that checks nothing |
| No next command, no timings, no scoping beyond a base ref | The agent re-runs everything, and nobody can measure what the gate costs |

## Proposal

### 1. Mission and principles

**Mission.** Garita is the deterministic gate for agent-authored change: it runs the checks a change
must pass at the narrowest scope that covers the risk, holds the gate itself to declared budgets, and
returns one machine-first verdict that agents, hooks and CI branch on without re-reading prose.

**Principles**, each inherited from urzua where urzua already holds it:

- **Machine-first.** stdout is exactly one JSON object; everything else goes to stderr. The exit code
  stays the gate (urzua RFC-3).
- **No silent no-ops.** A check that examined nothing is a finding, never a pass. A trigger, scope or
  glob that matches nothing is reported.
- **Deterministic by default.** A model-judged check warns. It blocks only where a declared risk
  class requires it (RFC-2), and garita's own part stays deterministic: it checks that the verdict
  exists, is pinned to the tree and says pass.
- **Fail closed on garita, fail open on infrastructure.** A broken garita install or config blocks, and
  so does a tool that cannot be started (a missing binary, a command that does not resolve): exit 1.
  A tool that started and then crashed or timed out is a `warn` finding naming the crash, never a
  silent pass; `on_crash: fail` makes it block.
- **Orchestrate, don't replace.** Garita calls Turbo, cargo, oxlint, tsc, Vitest and urzua. It does not
  cache build outputs or compute a package graph itself.
- **Opt-in configuration.** A repository with no `.garita/config.yaml` gets nothing it did not ask for.

### 2. The model: checks, triggers, scopes and cost

A **check** is a command plus a parser. A **trigger** is where it runs: `commit`, `push`, `ci`,
`merge`. A **scope** is what it runs over:

| Scope | Files or packages it receives | Typical trigger |
|---|---|---|
| `staged` | The staged files that match the check's globs | commit |
| `changed` | Files changed against the merge base with the target branch | push |
| `affected` | Packages affected by the change, from the orchestrator (`turbo --affected`, cargo's workspace) | push, ci |
| `all` | Everything | ci, merge |

Each check and each trigger declares a **budget**, and a budget is a gate in its own right (§4).

### 3. Configuration

```yaml
schema_version: 1
requires: ">=0.1.0"          # the oldest garita that may run this config

triggers:
  commit: { scope: staged,   budget: { duration: 3s } }
  push:   { scope: affected, budget: { duration: 60s } }
  ci:     { scope: all,      budget: { duration: 10m } }
  merge:  { scope: all, worktree: merged }

budgets:
  on_exceed: demote          # warn | fail | demote
  regression: { window: 20, threshold: 25% }

on_crash: warn              # warn | fail

merge:
  mode: status               # admin | status | auto
  method: squash

evidence:
  accept: ci                 # ci | local | any

checks:
  lint:
    run: oxlint {files}
    parser: oxlint
    globs: ["**/*.{ts,tsx}"]
    budget: { duration: 1s }
    on: [commit, push, ci]
  format:
    run: oxfmt --check {files}
    parser: oxfmt
    budget: { duration: 1s }
    on: [commit, push, ci]
  types:
    run: turbo type-check {affected}
    parser: tsc
    budget: { duration: 20s }
    on: [push, ci]
  test:
    run: turbo test {affected}
    parser: vitest
    budget: { duration: 45s }
    on: [push, ci]
  records:
    run: urzua check
    parser: urzua
    budget: { duration: 1s }
    on: [commit, ci]
  clean-tree:
    builtin: clean-tree
    on: [push, ci, merge]
```

Every key above is a configuration point with a default; a repository declares only what it changes.

`{files}` and `{affected}` are filled from the trigger's scope. A check with `{files}` and no matching
files is skipped with a `no-input` finding at `info`, so the skip is visible, and a check whose parser
reports zero files examined when it was given files is a `warn`.

The `staged` scope reads files from the working tree, not the index. A staged file that also has
unstaged changes is checked with those changes included, so garita reports it as a `partly-staged`
finding at `warn`.

### 4. Budgets are gates

Problems in an agentic stack rarely arrive as a failure. They creep in: a check grows from two seconds to
forty over a month, a test suite doubles, a hook that ran on staged files starts running on the tree.
Each step is too small to notice, and the sum is agents waiting, CI minutes spent and money burned.
Garita treats a limit the same way it treats a lint rule: crossing it is a finding.

- **Budget findings.** A check or trigger that runs over its budget produces a `budget` finding, with
  the measured value, the limit and the receipt that proves it. `on_exceed` decides what it does: `warn`
  reports it, `fail` blocks, and `demote` moves the check to the next trigger (commit to push, push to
  CI). A check that has run cannot be un-run, so the over-budget run reports the finding and the
  demotion takes effect from the next run: garita reads the last receipt for that check and trigger,
  skips the check there with a `demoted` finding at `info`, and runs it at the later trigger. It
  stays demoted, visibly, until a run at its own trigger comes in under budget
  (`garita run <check> --trigger <trigger>` forces one). A check can override the repository's
  `on_exceed`.
- **Regression findings.** Receipts carry every check's duration, so garita compares a run against the
  median of the last `window` runs of that check on the same trigger. A rise past `threshold` is a
  `budget-regression` finding even while the check is still under its absolute budget: the creep is
  caught when it starts, not when it crosses the line.
- **More than time.** A budget is a limit on any metric a check reports. Duration is built in; a
  parser or plugin can report others (files examined, tests run, bundle size, dependency count) and a
  budget can name them. The mechanism is the same: measured against a limit and against its own
  history.
- **Budgets are visible.** `garita budgets` prints each check's limit, its current median and its trend
  from receipts, so a slow check is a number someone can point at.

### 5. The output contract

```json
{
  "schema_version": 1,
  "status": "fail",
  "trigger": "push",
  "commit": "1b29de5ea",
  "tree": "9c1e4f2ab",
  "scope": { "kind": "affected", "packages": ["frontend"] },
  "checks": [
    {
      "name": "lint",
      "status": "fail",
      "duration_ms": 412,
      "examined": { "files": 38 },
      "findings": [
        {
          "severity": "error",
          "rule": "oxlint/no-debugger",
          "file": "ts/frontend/src/App.tsx",
          "line": 1302,
          "message": "Unexpected `debugger` statement",
          "fix": "oxlint --fix ts/frontend/src/App.tsx"
        }
      ],
      "rerun": "garita run lint --files ts/frontend/src/App.tsx"
    }
  ],
  "next_action": "garita run lint --files ts/frontend/src/App.tsx",
  "receipt": ".garita/receipts/20261121T031502Z-push-7f3a9c.json"
}
```

- `status` is `pass`, `warn` or `fail`; exit codes are `0`, `0` and `2`, with `1` reserved for garita's
  own errors, which always block.
- `next_action` is the narrowest command that re-runs what failed: the failing check, on the failing
  files or package. It is `null` on a pass.
- `examined` is what the check actually covered, one count per unit (`files`, `tests`, `packages`),
  which is how a no-op is caught.
- `commit` is `null` at the commit trigger, where no commit exists yet. `tree` is always present.
- `--format sarif` and `--format junit` render the same result for code scanning and CI summaries. JSON
  is the source; the others are views of it.

### 6. Parsers

A parser turns a tool's own output into findings. The point of garita is that the common tools have
real parsers, so a finding carries file, line, rule and message:

- **First set:** oxlint, oxfmt, tsc, Vitest, cargo (check, clippy, test), urzua, and a generic
  `exit-code` parser that keeps the tool's last lines of output as the finding's message, so an
  unparsed check still tells the agent more than an exit status.
- **Prefer the tool's own JSON** (`oxlint --format json`, `cargo --message-format json`, Vitest's JSON
  reporter). Scrape text only where a tool has no structured output.
- **Plugins** are a subprocess protocol: JSON in on stdin, a check result out on stdout, with `--describe`
  for the plugin's name, inputs and schema version. A parser outside the first set is a plugin, not a
  change to garita.

### 7. Surfaces

| Surface | How it runs | What it adds |
|---|---|---|
| Git hooks | `garita hooks install` writes `pre-commit` and `pre-push` that call `garita run --trigger <hook>`, into `core.hooksPath` when one is set | The same gate for people and agents; a fresh worktree reports a missing hook through `doctor` |
| Agent hook | A `PreToolUse` hook on the agent's shell tool that runs the trigger for `git commit` and `git push`, and blocks `--no-verify` | The agent gets the JSON verdict and `next_action` at the moment it tries to commit |
| CI | `garita run --trigger ci --format sarif` | Same config, same verdict; no workflow re-lists the checks |
| Merge | `garita merge <PR>`: merges the PR into the target branch in a throwaway worktree, runs the `merge` trigger there, acts on `pass` as `merge.mode` says, then removes the worktree | Gates the merged result, not the branch, and works when CI is unavailable |

An installed git hook exits non-zero, with a message, when `garita` is not on the machine or is older
than the config's `requires`. A machine without garita must not read as a pass.

`merge.mode` decides what a green merge gate does. `status` posts a `garita/merge` commit status on the
PR's head, which branch protection can require, and merges nothing. `auto` posts the status and turns on
the forge's auto-merge, so the PR merges once every required check is green. `admin` merges the PR
with admin rights, bypassing required checks: for a repository whose CI is unavailable and whose
maintainers accept the local gate in its place. `status` is the default, because it is the only mode
that grants garita no authority the repository did not give it.

### 8. Receipts

Every run writes a receipt: the full JSON result plus the tree hash, the machine and garita's version.
Receipts live under `.garita/receipts/` (ignored by git) and, in CI, in the run's artifacts. Per urzua
RFC-3 §5, a run is self-dating and self-scoping, and the receipt is the history: garita keeps no log in
the repository.

A receipt is named for when it ran, `<UTC timestamp>-<trigger>-<run id>.json`, never for the commit.
At the commit trigger no commit exists yet, and a second run must not overwrite the first: the runs
are the history that budgets and demotion read. The tree hash, and the commit when there is one, are
fields inside. Each check's entry records the exact command, its scope and whether the working tree
was dirty, so a receipt can be cited as evidence of what ran.

A **local receipt** is one written on a developer's or an agent's machine; a **CI receipt** is one
written by a CI run. A local receipt is a file anyone on that machine can edit, so it proves only what
that machine claims. It is enough for the agent on that machine and for decision-agent-kit reading the
same session. It is weak as lasting evidence. `evidence.accept` decides which receipts urzua's `gate:`
locator accepts: `ci` (the default), `local` or `any`. Trusting local receipts more needs attestation,
which urzua already scopes as a later phase.

Receipts are what make budgets work (§4) and what make the ecosystem compose (§9).

### 9. How garita fits with urzua and decision-agent-kit

| | Question | How it answers |
|---|---|---|
| **urzua** | Was it decided, and is the claim backed? | Checks records; never judges the change |
| **garita** | Does the change pass its checks? | Runs tools; deterministic verdict |
| **decision-agent-kit** | Is the turn really done? | Asks a decision model; advises, never blocks |

1. **urzua runs inside garita.** `urzua check` is a check with the `urzua` parser; its findings already
   carry the shape garita emits, so nothing is re-interpreted.
2. **Receipts become urzua evidence.** urzua gains a `gate:` locator: `gate:test@<commit>` resolves to a
   passing receipt for that check at that commit. A milestone's acceptance check or a spec's
   `Realized-by` can cite it, and urzua checks it like any other locator.
3. **decision-agent-kit reads receipts instead of inferring.** Its stop check reconstructs from the
   transcript which checks ran, whether they passed, and whether they ran after the last edit. A
   receipt states all three, so the rules become exact and the model is asked only about the reply.
4. **Model checks are advisory in garita.** A decision-agent-kit check can be a garita check with
   `advisory: true`; its findings are capped at `warn`, so a gate's result stays reproducible. The
   exception is a check a declared risk class requires (RFC-2).
5. **One harness adapter layer, eventually.** urzua and decision-agent-kit each ship agent adapters;
   garita needs the same. Garita starts with its own Claude Code adapter and moves to a shared one only
   when all three need the same events.

### 10. What garita leaves to other tools

- **Caching and the package graph** stay with Turbo and cargo. Garita passes `--affected`; it does not
  hash inputs.
- **Fixing** stays with the tools. A finding's `fix` names the tool's own fix command; garita does not
  rewrite files, and the `clean-tree` check holds every check to that.
- **Record semantics** stay with urzua.

### 11. Language and packaging

Rust, as a single binary, matching urzua: no runtime on the machine, parallel checks, fast start for a
hook that runs on every commit. Distributed through crates.io, prebuilt binaries for macOS (arm64 and
x64) and Linux (x64 and arm64), and an npm wrapper for JavaScript repositories.

### 12. Phases

- **Phase 0:** `run`, `doctor`, the config, the output contract, receipts, duration budgets with
  `on_exceed`, parsers for oxlint, oxfmt, tsc and `exit-code`, git hooks. Done when garita replaces the
  hand-written commit and push hooks of one TypeScript monorepo, and the old hooks are deleted.
- **Phase 1:** the agent hook, `merge` with its three modes, regression findings and `garita budgets`,
  the Vitest, cargo and urzua parsers, SARIF. Done when garita gates a Rust repository and a TypeScript
  repository in CI with the same contract.
- **Phase 2:** urzua's `gate:` locator, decision-agent-kit reading receipts, and budgets on metrics other
  than duration.

## Decided in review

- **Config format:** YAML, matching `.urzua/config.yaml`.
- **Merge authority:** a configuration point, `merge.mode` (§7), defaulting to `status`.
- **Local receipts as evidence:** a configuration point, `evidence.accept` (§8), defaulting to `ci`.
- **A check over budget:** a finding, and `on_exceed: demote` moves it to a later trigger (§4).
- **The examined count:** an object with one count per unit, a key shared with urzua (RFC-2).

## Open questions

1. **Regression baseline across machines.** A check's median on a laptop and on a build server differ.
   Is the regression window per machine, per trigger, or both?
2. **Where CI receipts are kept.** Run artifacts expire. Is a CI receipt's lasting home a check run, a
   PR comment, or a store outside the forge?

## Non-goals

- A build system, a cache or a package graph.
- A linter or formatter of its own.
- Hosted dashboards or a service. Receipts make one possible; it is not garita's scope.
- Judging records. That is urzua.
- Model-judged blocking outside a declared risk class (RFC-2). Anywhere else a model advises through
  garita and never blocks.

## References

- urzua ADR-101: the mission and principles this RFC inherits.
- urzua RFC-3: the stdout/stderr split, `status`, and the per-run result as history.
- urzua RFC-27: the engine's boundary, which puts judging the change outside urzua and so inside garita.
- decision-agent-kit SPEC-1: the stop check that receipts make exact.
