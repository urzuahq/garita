---
Stable-Id: 01M7XKPYXGR0XNEG2CTRGY6WRG
Status: Accepted
Date: 2026-11-21
Accepted-on: 2026-11-25
Author: beauwilliams
---
# 1 — garita: a machine-native gate for agent-authored change

> **Accepted, 2026-11-25.** The maintainer accepted RFC-1, RFC-2 and RFC-4 together, in answer to
> "garita RFC-1, RFC-2, RFC-4: accept?": "I will accept each of the three RFCs. Let's go ahead with
> that." (spoken and transcribed). RFC-3 was not part of that answer.

## Summary

garita runs the checks a change must pass (lint, format, types, tests, record checks, anything with an
exit code) and returns one verdict that an agent, a git hook and CI all read the same way. It is
declared once, in `.garita/config.yaml`, and run from four surfaces: commit, push, CI and merge.
Its output is a run report of the agent CLI contract, which agent-cli-kit owns (RFC-1 for the base and
the gate profile, RFC-3 for the run profile). garita implements that contract and defines none of it.

garita is the control plane for the checks on a change: it decides what runs, where and within what
budget, and gives the verdict. The tools it runs do the judging. That is the line between it and
urzua, which checks the records a change traces to and never the change itself. The two compose through the shared contract and through
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

**Mission.** garita is the control plane that gates agent-authored change: it runs the tools that
check a change at the narrowest scope that covers the risk, holds the gate itself to declared budgets, and
returns one machine-first verdict that agents, hooks and CI branch on without re-reading prose.

**Principles**, each inherited from urzua where urzua already holds it:

- **Machine-first.** The report is one JSON object on stdout (§5), and the exit code stays the gate
  (urzua RFC-3).
- **No silent no-ops.** A check that examined nothing is a finding, never a pass. A trigger, scope or
  glob that matches nothing is reported.
- **Deterministic by default.** A model-judged check warns. It blocks only where a declared risk
  class requires it (RFC-3), and garita's own part stays deterministic: it checks that the verdict
  exists, is pinned to the tree and says pass.
- **Fail closed on garita, fail open on infrastructure.** A broken garita install or config blocks, and
  so does a tool that cannot be started (a missing binary, a command that does not resolve) or a
  native tool that reports it could not run (RFC-2): `not-run`, exit 2.
  A tool that was killed or timed out gave no verdict of its own: that is a `crash` finding at
  `warning`, never a silent pass, and `on_crash: fail` makes it block. A tool that exited non-zero
  did give a verdict, so it blocks, even when garita cannot read what it said and even when every
  finding it printed is a warning: a non-zero exit never becomes a pass. The one exception is a
  tool saying it had no files to check, which is reported as a run over nothing.
- **A missing config is not a pass.** With no `.garita/config.yaml`, `garita run` is `not-run`, exit
  2, and its `error` names the missing file. An unparseable config, a config that fails its schema
  and an unknown key are the same.
- **Orchestrate, don't replace.** garita calls Turbo, cargo, oxlint, tsc, Vitest and urzua. It does not
  cache build outputs or compute a package graph itself.
- **Opt-in configuration.** garita installs nothing on its own: a repository has a gate only once it
  adds a config and runs `garita hooks install`. Every key has a default, and a repository declares
  only what it changes.

### 2. The model: checks, triggers, scopes and cost

A **check** is a command plus a parser. It has a **kind** (`lint`, `format`, `types`, `test`,
`build`, `records` or `other`), which its adapter supplies and the config can override, because a
reader needs to know what a pass proves: a passing lint does not verify a claim about behaviour.

garita runs a check's command directly, with no shell, so the exit code it records is the tool's own.
A pipeline hides a failing tool behind the exit code of its last command, and a hand-written hook
rarely sets `pipefail`. A check that needs a shell names one in its command and takes that risk
openly. A `run` string is split into arguments by POSIX shell quoting rules, with no expansion of
variables, globs or operators, so `sh -c "a | b"` is three arguments. `{files}` expands to one argument per file. `{affected}` hands the question to the task runner: it
is valid only with `via: turbo`, and expands to `--affected` when the scope is `affected` and to
nothing otherwise, with the base ref passed to the runner. A placeholder is an argument on its own,
quoted or not, and `#` starts no comment.

A **trigger** is where it runs: `commit`, `push`, `ci`, `merge`. A **scope** is what it runs over:

| Scope | Files or packages it receives | Typical trigger |
|---|---|---|
| `staged` | The staged files that match the check's globs | commit |
| `changed` | Files changed against the merge base with the target branch | push |
| `affected` | The changed files for `{files}`; for `{affected}`, the packages the task runner computes (`turbo --affected`) | push, ci |
| `all` | Everything | ci, merge |

The base ref for `changed` and `affected` is `--base` when given, else the first of `origin/HEAD`,
`origin/main`, `origin/master`, `main` and `master` that exists. A local branch is passed over when
it is the branch being checked or sits at the same commit, since a branch compared with itself has
no changes and would pass anything. A remote branch at the same commit is used: nothing is ahead of
the remote. A `--base` given by hand is used as it is, with a `base-is-head` finding at `info` when
it names the commit being checked. When a scope cannot be
computed (a shallow clone, no base ref, no merge base), garita widens it to `all` and reports a
`scope-widened` finding at `info`. It never narrows on a guess.

A run in which no check applies to the trigger reports a `no-checks` finding at `warning`. A long
file list is split across several runs of the tool, so a large change cannot exceed the operating
system's limit on a command line.

`garita run --files <paths>` replaces the trigger's scope with the files given, and
`garita run <check>` runs the named checks whatever their `on` list says. Findings that belong to
the run and not to a check (`scope-widened`, `partly-staged`, a trigger's `budget`) carry no `check`
(§5).

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
timeout: { factor: 10 }     # a check is killed at this multiple of its budget
receipts: { keep: 200 }

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
    via: turbo
    budget: { duration: 20s }
    on: [push, ci]
  test:
    run: turbo test {affected}
    parser: vitest
    via: turbo
    budget: { duration: 45s }
    on: [push, ci]
  records:
    native: urzua
    budget: { duration: 1s }
    on: [commit, ci]
  clean-tree:
    builtin: clean-tree
    on: [push, ci, merge]
```

Every key above is a configuration point with a default; a repository declares only what it changes.
Only `schema_version` is required. The defaults that matter most:

- A check takes one of three forms: `run` with a `parser`, `native` (RFC-2), or `builtin`. A `run`
  with no `parser` uses `exit-code`. The one builtin is `clean-tree`.
- A check with no `on` runs at every trigger.
- A check reports every finding its tool gives unless it sets `min_severity` (`error`, `warning` or
  `info`). Findings below it are left out of the verdict and counted in the check's `suppressed`
  (agent-cli-kit RFC-3 §3), so a repository with a backlog of warnings can keep them out of an
  agent's way without hiding that they exist. garita's own findings about a check are never suppressed.
- A check runs from the repository root unless it sets `cwd`, a directory inside the repository.
  Files outside a check's `cwd` or its partition roots are left out of that check, and an
  `outside-root` finding at `info` says so.
- A trigger with no `scope` uses `staged` at commit, `changed` at push, and `all` at ci and merge.
- `budgets.on_exceed` and `on_crash` are `warn`, `timeout.factor` is 10 and `receipts.keep` is 200.
- An unknown key anywhere is an error, and every problem in a config is reported at once, each with
  the path of its key.

`{files}` and `{affected}` are filled from the trigger's scope. A check with `{files}` and no matching
files is skipped with a `no-input` finding at `info`, so the skip is visible, and a check whose tool
examined nothing gets a `no-op` finding: a `warning` when the tool exited zero, an `error` when it
did not. A check with no placeholder always runs, and what it examined is whatever its adapter
reports (RFC-2).

The `staged` scope reads files from the working tree, not the index. A staged file that also has
unstaged changes is checked with those changes included, so garita reports it as a `partly-staged`
finding at `warning`.

### 4. Budgets are gates

Problems in an agentic stack rarely arrive as a failure. They creep in: a check grows from two seconds to
forty over a month, a test suite doubles, a hook that ran on staged files starts running on the tree.
Each step is too small to notice, and the sum is agents waiting, CI minutes spent and money burned.
garita treats a limit the same way it treats a lint rule: crossing it is a finding.

- **Budget findings.** A check or trigger that runs over its budget produces a `budget` finding, with
  the measured value, the limit and the receipt that proves it. `on_exceed` decides what it does: `warn`
  reports it at `warning`, `fail` reports it at `error`, and `demote` reports it at `warning` and moves the check to a
  later trigger.
- **Demotion.** Only a check with its own budget can be demoted, and only one whose tool exited on
  its own and passed or warned. A check that failed, or whose tool was killed at its timeout, runs
  again at the same trigger however slow it was, so being slow or hanging never releases a check. The later trigger is the next later one in the check's own `on` list, whatever order
  the list is written in: a check with `on: [commit, ci]` demotes from commit to ci. A check that has run cannot be un-run, so the
  over-budget run reports the finding and the demotion takes effect from the next run. At each run
  garita reads the last receipt in which that check actually ran at this trigger; if that run was
  over budget, the check is skipped here with a `demoted` finding at `info`. It stays demoted,
  visibly, until `garita run <check> --trigger <trigger>` runs it here under budget. With no later
  trigger in its `on` list, `demote` acts as `warn`. A trigger's own budget cannot demote anything:
  for a trigger, `demote` acts as `warn`. Demotion is read from this machine's receipts, so it is
  per machine: a check demoted on a slow laptop still runs at commit elsewhere. A check can override
  the repository's `on_exceed`.
- **Timeouts.** A budget measures; a timeout kills. A check's timeout is `timeout.factor` times its
  budget. A check with no budget uses `timeout.factor` times its trigger's budget, and with neither,
  ten minutes. A check killed at its timeout is a crash (`on_crash`), never a pass.
- **Regression findings.** Receipts carry every check's duration, so garita compares a run against the
  median of the last `window` runs of that check on the same trigger. A rise past `threshold` is a
  `budget-regression` finding even while the check is still under its absolute budget: the creep is
  caught when it starts, not when it crosses the line.
- **More than time.** A budget is a limit on any metric a check reports. Duration is built in; a
  parser or plugin can report others (files examined, tests run, bundle size, dependency count) and a
  budget can name them. The mechanism is the same: measured against a limit and against its own
  history.
- **Budgets are visible.** `garita budgets` reports each check's limit, its current median and its
  trend from receipts, as a JSON report like every other garita command's, so a slow check is a
  number an agent can read.

### 5. Output

garita's output is a run report. Its shape is not garita's: agent-cli-kit owns it, and garita
implements it. The base keys, the exit codes and the rules for `not-run` are agent-cli-kit RFC-1 §1.
`blocking`, `findings`, `examined` and the fields of a finding are its §2. The run profile is
agent-cli-kit RFC-3: the report's `checks` and the fields of each entry (§1), `trigger`, `commit`,
`tree`, `scope` and `receipt` (§1 and §2), `suppressed` (§3), `check` on a finding (§4) and the
`<source>/<rule>` form of a wrapped tool's rule id (§5). agent-cli-kit RFC-4 adds the rest: an
empty stderr, `error_kinds`, the argv forms of `next_action` and `fix`, a check's `output_tail` and
`output`, what `describe` declares about a command's inputs, and what a hook prints. This record
repeats none of that. It states
what garita does with those keys, and, under garita's extended contract below, what garita adds
beyond them.

**One JSON object, nothing else.** Every command prints exactly one JSON object on stdout and
nothing on stderr, except a diagnostic when garita cannot write that object. A command line garita
cannot read, one that names no command included, and an internal failure are each a `not-run`
report. Only `--help` and `--version` print the argument parser's own text.

The report below illustrates a run report. It defines nothing.

```json
{
  "schema_version": 1,
  "status": "fail",
  "blocking": true,
  "trigger": "push",
  "commit": "1b29de5ea3f04c7d9a6b2e8f5c1d0a7b3e9f4c62",
  "tree": "9c1e4f2ab07d5e3c8f1a6b4d2e0c9f7a5b3d1e80",
  "scope": { "kind": "affected", "packages": ["frontend"] },
  "checks": [
    {
      "name": "lint",
      "kind": "lint",
      "status": "fail",
      "duration_ms": 412,
      "examined": { "files": 38 },
      "rerun": "garita run --trigger push lint --files ts/frontend/src/App.tsx"
    }
  ],
  "findings": [
    {
      "check": "lint",
      "severity": "error",
      "rule": "oxlint/no-debugger",
      "file": "ts/frontend/src/App.tsx",
      "line": 1302,
      "column": 5,
      "message": "Unexpected `debugger` statement",
      "fix": "oxlint --fix ts/frontend/src/App.tsx",
      "fingerprint": "5d1c0e9a"
    }
  ],
  "examined": { "files": 38 },
  "next_action": "garita run --trigger push lint --files ts/frontend/src/App.tsx",
  "next_action_reason": "1 error in 1 check",
  "next_action_inputs": [],
  "receipt": ".garita/receipts/20261121T031502Z-push-01M7XM3Q4T8Z6R2K9V5B1N0HCW.json"
}
```

What garita does with the report:

- **Which status a run gets, and the exit code.** The words a run and a check may have are the run
  profile's (agent-cli-kit RFC-3 §1, version 1.1), and this record does not define them. A
  check is `skipped` when it did not run, with a `no-input` or `demoted` finding saying why. `pass`
  and `warn` exit 0, `fail` exits 1 and is the only blocking status, and `not-run` exits 2: a broken
  install or config, or a tool that cannot be started. A hook stops the commit on 1 and on 2.
- **One list of findings.** `findings` holds every finding from every check, each naming its
  `check`. A finding about the run itself, which no check produced (`scope-widened`,
  `partly-staged`, a trigger's `budget`), carries no `check`, as the run profile says. The
  top-level `examined` is the total across checks.
- **When outcomes mix, `not-run` wins.** If any check could not start, the run is `not-run`, exit 2
  and `blocking` false, whatever else failed, because a gate that did not fully run has no verdict.
  Otherwise a failed check or any finding at `error` makes it `fail`, a check that warned or any
  finding at `warning` makes it `warn`, and a run with neither is `pass`. A check's own status
  counts even when it produced no finding, so a tool that failed without saying why still blocks.
  Every finding is still listed.
- **A `not-run` report has the same keys as any other.** The gate profile lets a command that could
  not run leave out `blocking`, `findings` and `examined`. garita always prints them: `blocking`
  false, the other two empty when nothing ran, plus an `error` saying why. A reader never has to
  branch on whether they exist.
- **A check that examined nothing.** A check whose `examined` counts are all zero is a `no-op`
  finding (§3), never a pass.
- **`commit` is `null` at the commit trigger,** where no commit exists yet. `tree` is present
  whenever the run started in a repository; a run that could not start at all has none.
- **Suppression.** A check's `min_severity` (§3) decides what is counted in its `suppressed`.
- **Which kind a `not-run` is.** A command line garita cannot read is `usage`. A config that is
  missing, unreadable, invalid or asks for another garita is `config`. A directory that is not a
  git repository and a tool that cannot be started are `environment`. Receipts that cannot be
  stored do not stop a run: it keeps its verdict and gains a `receipt-unwritten` finding at
  `info`. A failure inside garita is `internal`.
- **A check's output.** `output` names the file holding everything the check's command printed,
  kept beside the receipt. `output_tail` is its end, printed for a check that failed, warned or
  could not run, with control characters other than newline and tab replaced; the file is the
  raw output. A finding's message for a failed command is one sentence and holds none of it. When
  the file cannot be written, `output` is left out and the tail is still printed.
- **What `garita describe` declares.** It prints the object agent-cli-kit RFC-1 §3 and RFC-4 define.
  It declares `run`, `doctor`, `hooks install` and `hooks uninstall`, each with profile `run`,
  including the three that select no checks, and each one's arguments. `run` declares the
  `base-ref` scope argument. It does not declare `paths`: its positional arguments are check
  names, so it takes files through `--files` (agent-cli-kit RFC-3 §6). Only `doctor` is declared
  safe. garita has no log and reads no standard input.

#### garita's extended contract

These are garita's own conventions and keys beyond agent-cli-kit's base and run profile. They are
not the shared contract, they contradict nothing in it, and a reader that ignores them loses nothing
the shared contract promises.

- **`next_action`.** After `garita run` it holds the narrowest command that re-runs what failed:
  the failing checks, at the same trigger, on the failing files when every one of them has files.
  Only a check is ever named as a check to re-run: an error that belongs to the run, such as a
  trigger over its budget, re-runs the whole trigger. When the run is `not-run` it is
  `garita doctor`, or `garita describe` after a usage error. It is `null` on `pass` and `warn`,
  and `null` from the `hooks` commands and from `doctor`, whose findings say what to change, except
  that `doctor` gives `garita hooks install` when that one command is the whole repair.
  `next_action_reason` is a count of what failed, such as `1 error in 1 check`.
- **`fingerprint`.** An opaque string, stable across runs for the same finding. How garita computes
  it is its own, and a reader must not parse it.
- **`message`.** Text for a reader that promises no layout. Output a reader should parse gets a key.
- **`output_notable`.** On a check's entry: up to a few lines from anywhere in its output that look
  like the reason it failed, which the tail can miss. Under a task runner each line starts with
  its task's name.
- **`examined` units.** garita uses `files`, `tests` and `packages`.
- **Rule ids.** The wrapped tools' ids follow the `<source>/<rule>` form of agent-cli-kit RFC-3 §5.
  The ids garita raises itself are its own, such as `no-op`, `no-input`, `demoted`, `budget`,
  `crash`, `scope-widened` and `partly-staged` from a run, and the `doctor/` ids from
  `garita doctor`. This record does not list them all.
- **`risk`, `system` and `verdict`.** Keys proposed in RFC-3 §6, not built. They are garita's until a
  second tool needs them and they are proposed to agent-cli-kit.
- **The plugin protocol** (§6) is garita's own.

### 6. Parsers

A parser turns a tool's own output into findings. The point of garita is that the common tools have
real parsers, so a finding carries file, line, rule and message:

- **First set:** oxlint, oxfmt, tsc, Vitest, cargo (check, clippy, test), and a generic
  `exit-code` parser, whose check entry carries the end of the tool's output (§5), so an unparsed
  check still tells the agent more than an exit status.
- **Prefer the tool's own JSON** (`oxlint --format json`, `cargo --message-format json`, Vitest's JSON
  reporter). Scrape text only where a tool has no structured output.
- **Read the output, not the exit code.** A tool can exit 1 for findings, for a bad config and for
  having matched no files. RFC-2 makes a parser one part of an adapter, which also says which of
  those happened.
- **Plugins** are a subprocess protocol of garita's own (its extended contract, §5): JSON in on
  stdin, a check result out on stdout, with `--describe` for the plugin's name, inputs and schema
  version. A parser outside the first set is a plugin, not a
  change to garita.

### 7. Surfaces

| Surface | How it runs | What it adds |
|---|---|---|
| Git hooks | `garita hooks install` writes `pre-commit` and `pre-push` that call `garita run --trigger <hook>`, into `core.hooksPath` when one is set | The same gate at every commit and push; a fresh worktree reports a missing hook through `doctor` |
| Agent hook | A `PreToolUse` hook on the agent's shell tool that runs the trigger for `git commit` and `git push`, and blocks `--no-verify` | The agent gets the JSON verdict and `next_action` at the moment it tries to commit |
| CI | `garita run --trigger ci` | Same config, same verdict; no workflow re-lists the checks |
| Merge | `garita merge <PR>`: merges the PR into the target branch in a throwaway worktree, runs the `merge` trigger there, acts on `pass` as `merge.mode` says, then removes the worktree | Gates the merged result, not the branch, and works when CI is unavailable |

An installed git hook blocks when `garita` is not on the machine or is older than the config's
`requires`: it exits 2 and prints a `not-run` report, as agent-cli-kit RFC-4 says a hook must. A
machine without garita must not read as a pass.

`garita doctor` says whether the gate will run here: a missing, foreign or outdated hook, a tool a
check names that is not on the machine, receipts that git does not ignore, `garita` itself missing
from `PATH`. It changes nothing. `garita hooks install` never overwrites a hook it did not write
unless told to with `--replace`, and writes both hooks or neither.

From Phase 1, `doctor` also reports a target branch that does not require garita's status before
merging, and a merge queue whose CI does not run on the queue's merged result. In both cases a check
that looks required never gates.

`merge.mode` decides what a green merge gate does. `status` posts a `garita/merge` commit status on the
PR's head, which branch protection can require, and merges nothing. `auto` posts the status and turns on
the forge's auto-merge, so the PR merges once every required check is green. `admin` merges the PR
with admin rights, bypassing required checks: for a repository whose CI is unavailable and whose
maintainers accept the local gate in its place. `status` is the default, because it is the only mode
that grants garita no authority the repository did not give it.

### 8. Receipts

Every run writes a receipt: the report it printed, plus what is needed to say exactly what that
report is evidence for. A result is only evidence for the tree, the configuration and the tools that
produced it. What a receipt holds is agent-cli-kit's: its field list and the vocabulary of its
`result` are in agent-cli-kit RFC-1 §5 and `receipt.schema.json`, where the receipt is still a
draft, and this record does not restate them.

A run that was `not-run` writes a receipt too, with `result` `FAILED`: a gate that could not run is
not evidence of a pass. Older receipts are deleted after the new one is written, so a
`receipts-unpruned` finding is in the printed report and not in the receipt's copy of it.
Receipts live under `.garita/receipts/` (ignored by git) and, in CI, in the run's artifacts. Per urzua
RFC-3 §5, a run is self-dating and self-scoping, and the receipt is the history: garita keeps no log in
the repository.

A receipt is named for when it ran, `<UTC timestamp>-<trigger>-<run id>.json`, never for the commit.
At the commit trigger no commit exists yet, and a second run must not overwrite the first: the runs
are the history that budgets and demotion read. The tree hash, and the commit when there is one, are
fields inside. garita keeps the last `receipts.keep` receipts and deletes older ones, except the
last receipt in which each check ran at each trigger, which demotion reads (§4).

Each check's entry records the exact command, the tool's own exit code and its times, so a receipt
can be cited as evidence of what ran. The full output of a check is kept beside the receipt and
deleted with it. `examined` says whether the check looked at anything, so a test run that selected
no tests does not read as a pass. `worktree` lets a reader tell exactly whether a check ran after the
last edit: it did if the working files still hash to the receipt's `worktree`. That answer does not
depend on comparing clocks, and it is the one decision-agent-kit's stop check needs (§9).

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

1. **urzua runs inside garita.** `urzua check` is a native check (RFC-2): it speaks the contract, so
   garita reads its report unchanged.
2. **Receipts become urzua evidence.** urzua gains a `gate:` locator: `gate:test@<commit>` resolves to a
   passing receipt for that check at that commit. A milestone's acceptance check or a spec's
   `Realized-by` can cite it, and urzua checks it like any other locator.
3. **decision-agent-kit reads receipts instead of inferring.** Its stop check reconstructs from the
   transcript which checks ran, whether they passed, and whether they ran after the last edit. A
   receipt states all three, so the rules become exact and the model is asked only about the reply.
4. **Model checks are advisory in garita.** A decision-agent-kit check can be a garita check with
   `advisory: true`; its findings are capped at `warning`, so a gate's result stays reproducible. The
   exception is a check a declared risk class requires (RFC-3).
5. **One harness adapter layer, eventually.** urzua and decision-agent-kit each ship agent adapters;
   garita needs the same. garita starts with its own Claude Code adapter and moves to a shared one only
   when all three need the same events.

### 10. What garita leaves to other tools

- **Caching and the package graph** stay with Turbo and cargo. garita passes `--affected`; it does not
  hash inputs.
- **Fixing** stays with the tools. A finding's `fix` names the tool's own fix command; garita does not
  rewrite files, and the `clean-tree` check holds every check to that.
- **Record semantics** stay with urzua.

### 11. Language and packaging

Rust, as a single binary, matching urzua: no runtime on the machine, parallel checks, fast start for a
hook that runs on every commit. Distributed as prebuilt binaries on GitHub releases (RFC-4), first for
Linux x64 and macOS on Apple Silicon. Publishing to crates.io and an npm wrapper for JavaScript
repositories waits on a decision for the whole organisation.

### 12. Phases

- **Phase 0:** `run`, `doctor`, the config, the output contract, receipts, duration budgets with
  `on_exceed` and timeouts, adapters (RFC-2) for oxlint, oxfmt, tsc and `exit-code`, git hooks. Done
  when garita replaces the hand-written commit and push hooks of one TypeScript monorepo, and the old
  hooks are deleted.
- **Phase 1:** the agent hook, `merge` with its three modes, regression findings and `garita budgets`,
  the Vitest and cargo adapters, urzua as a native check. Done when garita gates a Rust repository and a TypeScript
  repository in CI with the same contract.
- **Phase 2:** urzua's `gate:` locator, decision-agent-kit reading receipts, budgets on metrics other
  than duration, and systems and risk classes (RFC-3).

## Decided in review

- **Config format:** YAML, matching `.urzua/config.yaml`.
- **Merge authority:** a configuration point, `merge.mode` (§7), defaulting to `status`.
- **Local receipts as evidence:** a configuration point, `evidence.accept` (§8), defaulting to `ci`.
- **A check over budget:** a finding, and `on_exceed: demote` moves it to a later trigger (§4).
- **The examined count:** agent-cli-kit's `examined`, one count per unit, shared with urzua.
- **The output contract:** agent-cli-kit's, not garita's. garita implements it and keeps its own
  additions in an extended contract (§5).
- **garita is the control plane.** The tools it runs do the judging.

## Open questions

1. **Regression baseline across machines.** A check's median on a laptop and on a build server differ.
   Is the regression window per machine, per trigger, or both?
2. **Where CI receipts are kept.** Run artifacts expire. Is a CI receipt's lasting home a check run, a
   PR comment, or a store outside the forge?
3. **Checking the index.** The `staged` scope reads the working tree and warns on a partly staged
   file. Checking exactly what is staged means stashing unstaged changes first, which can lose work
   when a tool rewrites files. Is the warning enough?
4. **Only new findings.** A repository adopting garita may already have findings. Should a check be
   able to block only on findings the change introduced, matched by fingerprint against the base?
5. **Flaky checks.** A check that passes on a retry is not the same as one that passed. Should
   "passed after N retries" be its own status, and can a flaky check be quarantined (still run,
   unable to block) with an owner and an expiry?
6. **The cost of `worktree`.** Hashing the working files stores a git object for each changed or
   untracked file and runs its clean filter, on every run. That is cheap for source files and not
   for a large untracked file or a repository using large-file storage. It also does not see
   uncommitted changes inside a submodule. Should `worktree` skip files over a size, or be computed
   only when a reader asks for it?
7. **Noisy timings.** A regression rule of a fixed percentage over a median will fire on a busy
   machine. Does it need a statistical test, or a count of consecutive slow runs?

## Non-goals

- A build system, a cache or a package graph.
- A linter or formatter of its own.
- Hosted dashboards or a service. Receipts make one possible; it is not garita's scope.
- Judging records. That is urzua.
- Model-judged blocking outside a declared risk class (RFC-3). Anywhere else a model advises through
  garita and never blocks.

## References

- urzua ADR-101: the mission and principles this RFC inherits.
- urzua RFC-3: `status` and the per-run result as history.
- urzua RFC-27: the engine's boundary, which puts judging the change outside urzua and so inside garita.
- decision-agent-kit SPEC-1: the stop check that receipts make exact.
- agent-cli-kit RFC-1 (base, gate profile, `describe`, receipt) and RFC-3 (run profile): the output
  contract.
- RFC-2: adapters and native tools. RFC-3: systems and risk classes.
