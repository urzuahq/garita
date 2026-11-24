---
Stable-Id: 01M82HJ6S3BY3PXRGJW1NXH0XZ
Status: Draft
Date: 2026-11-22
Author: beauwilliams
---
# 2 — Adapters and native tools

## Summary

Every tool garita runs reaches it in one of two ways. A **native** tool speaks the agent CLI contract
itself, and garita reads its report unchanged. urzua is the first. Every other tool gets an
**adapter**: the part of garita that describes the tool, invokes it at a scope, parses its output
into findings and reports its metrics.

RFC-1's parsers become one part of an adapter. The input side that real hooks need (one run per
package directory, excluded paths, output that arrives through a task runner, checks that take no
files) becomes another part of the same interface.

## Motivation

**Every new tool costs a parser, and the parser is half of the work.** RFC-1 §6 lists the parsers
garita ships. Replacing a real repository's hooks showed that calling the tool is as much work as
reading it: a commit hook runs a linter once per package from inside the package, with paths rebased;
it leaves one package out; a push hook runs the type checker through a task runner that prefixes and
interleaves the output. These are one concern in two places.

**A tool that already speaks the contract should need neither.** urzua prints one JSON report with a
status, findings, what it examined and the next command. Translating that into the same shape is work
that can only lose information.

**An exit code is not a result.** A linter can exit 1 for findings, for a bad config and for having
matched no files. An adapter has to read what the tool said, and say which of the three happened.

## Proposal

### 1. The adapter interface

An adapter has four parts:

| Part | What it does |
|---|---|
| `identity` | The tool's name, its system (RFC-3), the kind of check it is, the scopes it accepts and the metrics it reports |
| `invoke` | Turns a trigger's scope into the tool's arguments and working directory |
| `parse` | Turns the tool's output into findings, reports what it examined, and classifies the run |
| `metrics` | The numbers a budget can limit (RFC-1 §4): duration, files examined, tests run |

A built-in adapter is code in garita. A plugin adapter is a subprocess that speaks the plugin
protocol, which is part of garita's extended contract (RFC-1 §5, §6), with `--describe` returning
the `identity` part. The `identity` part is garita's internal interface. It is not the `describe`
command of agent-cli-kit RFC-1 §3, which a native tool prints (§6).

### 2. Invoke

`invoke` owns everything about how a tool is called:

- **Placeholders.** `{files}` and `{affected}` are filled from the scope (RFC-1 §2). A check whose
  command has no placeholder always runs. That is not a `no-input` skip.
- **Flags the parser needs.** For a tool garita runs directly, an adapter appends the flags its
  parser depends on when the command does not carry them: `--format=json` for oxlint and
  `--pretty false` for tsc. A command that asks oxlint for a different format is a configuration
  error. For a tool behind a task runner (`via`), garita adds only the runner's own `--summarize`
  and `--continue`: the tasks are the repository's own scripts, which may not be the tool at all,
  so nothing is passed through to them.
- **`partition`.** One run per matching root, from inside it, with paths rebased to the root.
  Findings are mapped back to repository-relative paths.
- **`exclude`.** Globs removed from the check's input.
- **`via`.** The tool runs through a task runner, for example `via: turbo` (section 5).

```yaml
checks:
  lint:
    run: oxlint {files}
    parser: oxlint
    partition: { by: dir, roots: ["packages/*"] }
    exclude: ["packages/docs/**"]
  types:
    run: turbo type-check {affected}
    parser: tsc
    via: turbo
  records:
    native: urzua
```

### 3. Parse

`parse` returns three things:

- **Findings.** Each has `rule`, `severity`, `file`, `line`, `column`, `message`, the tool's own
  `fix` command when it has one, and a `fingerprint` that is stable across runs. The fields of a
  finding are agent-cli-kit RFC-1 §2's; how the fingerprint is computed is garita's own (RFC-1 §5).
- **What was examined.** A count per unit (`files`, `tests`, `packages`), taken from the tool's own
  output where it reports one. This is how a run over nothing is told apart from a pass.
- **The kind of run.** One of: clean, findings, ran on nothing, or crashed. The adapter decides this
  from the output and never from the exit code alone. A crash is a tool that was killed or timed
  out, or one that exited zero with output its adapter expected to read and could not (RFC-1's
  `on_crash`). A tool that exited non-zero with output the adapter could not read is
  not a crash: it is one finding at `error`, `<tool>/unrecognized-output`, and the check's entry
  carries the lines of its output that look like errors and the end of the output (RFC-1 §5). A new tool version that words
  something differently then blocks and says so, and never passes as a warning. A run over nothing
  is always a finding: a `no-op` warning when the tool exited zero, and an error when it did not.
  The oxlint adapter treats output it cannot read from a tool that exited zero as clean, since it
  reads plain text as well as JSON and a clean text run prints nothing it can recognise.

What the first adapters read, each settled from a recorded run:

| Tool | Source | Examined count | What the recording showed |
|---|---|---|---|
| oxlint | `--format json` | The report's `number_of_files` | Exit 1 covers findings, no files matched and a bad config alike. A bad config prints no JSON at all, and no files matched prints a line of prose before the JSON. The default format changes when the tool detects an agent, so the format is always passed |
| oxfmt | `--check` | The file count in its closing line | Exit 0 formatted, 1 unformatted, 2 for an error or no files matched. Paths only, so a finding has a file and no line or rule |
| tsc | `--pretty false` diagnostic lines | `--listFilesOnly`, leaving out the compiler's own library files | TypeScript 5.8 exits 1 or 2 for the same errors depending on build state, and the TypeScript 7 native compiler exits 1 where 5.8 exits 2, so only zero or non-zero means anything. Multi-line messages continue on indented lines. The two compilers print the same diagnostic lines |
| `exit-code` | The exit code and the last lines of output | The files passed to it | The check's entry carries the end of the output |

Paths in findings (oxlint's `filename`, oxfmt's listed files, tsc's diagnostic lines) are relative to
the working directory, so a partitioned run rebases them from the root it ran in. Other output is
not: `--listFilesOnly` prints absolute paths, and so do some error messages.

**Each adapter is written against a recorded run of the real tool,** and the recordings are its test
fixtures, kept under the tool's version. Documentation is not enough: the facts an adapter depends
on (exit codes, the text for no files matched, what paths are relative to) are mostly undocumented.

### 4. Metrics

Duration is measured by garita for every check. An adapter can report others, and a budget can name
any of them (RFC-1 §4). The examined counts are metrics too, so a check that suddenly examines far
fewer files than its history is visible the same way a slow one is.

### 5. Tools behind a task runner

With `via`, the command garita runs is the runner's, and the runner runs the tool once per package.
The adapter needs, per task: the package directory, the exit code and that task's output, which it
hands to `parse` with paths rebased from the package directory.

For turbo, the adapter runs it with `--summarize` and `--continue` and reads the run summary it
writes. The summary gives each task's package directory, exit code and log file, with paths relative to
the workspace root. The log file holds that task's output without turbo's prefix, together with the
package manager's own lines (the command it ran, and a line reporting a failed exit), which `parse`
ignores. Without `--continue` a failure can stop the other tasks, and which of them then appear in
the summary depends on timing, so the adapter always passes it and takes its list of tasks from the
summary, never from the log files. It never parses turbo's prefixed terminal output, whose order
changes from run to run.

A runner returns the highest exit code among its tasks, so it cannot tell findings from a crash. The
adapter classifies each task separately, and a finding with no file names its task.

- **Tool output is read as the task printed it.** Behind a runner the oxlint adapter reads oxlint's
  ordinary text output as well as its JSON, since it cannot ask for JSON. A task that exits zero with
  output the adapter cannot read is a pass: it may be another tool sharing the task name.
- **A task with no log.** A runner writes no log for a task it does not cache. Such a task passes
  when it exited zero, and when it did not, its finding carries the runner's own output.
- **One at a time.** Checks behind a task runner run strictly one after another, and each accepts
  only a run summary that holds its own tasks. Two of them running together in one directory could
  otherwise read each other's results. The runner still runs its own tasks in parallel.

### 6. Native tools

A tool is **native** when it meets the agent CLI contract, which agent-cli-kit owns (RFC-1, and RFC-3
for a tool that runs other checks), and has a `describe` command (RFC-1 §3). garita reads its report
and its `describe` as those records define them.

- A native check is declared with `native: <tool>` in place of `run` and `parser`. garita passes it
  the scope and reads its report unchanged.
- **Version check.** garita reads the tool's `describe` once per run and compares the major of its
  `contract_version` with its own. A different major makes the check `not-run`, with a finding that
  names both versions. garita never guesses at a report it does not understand. Unknown keys from a
  newer minor are ignored.
- **A native tool that exits 2 could not run** (agent-cli-kit RFC-1 §1). The check is `not-run`, the same as a tool that
  cannot be started, and so is the garita run (RFC-1 §5). It is not a crash.
- **urzua is the first native tool.** The contract is written from its report. What urzua lacks is
  the `describe` command and the `examined` object, which it adds beside its existing
  `files_examined` in a minor release (agent-cli-kit RFC-1 §7). Nothing in its output changes or is
  removed.

garita is itself a native tool: its own output meets the contract, so one garita can run another, and
urzua or decision-agent-kit can read a garita report without an adapter. Its own `describe` is
declared in RFC-1 §5.

## Phases

- **Phase 0:** the adapter interface with `partition`, `exclude` and `via`; adapters for oxlint,
  oxfmt, tsc and `exit-code`, each written against the recordings.
- **Phase 1:** native tools, the version check, urzua as the first native check, and plugin
  adapters; adapters for Vitest and cargo.

## Decided so far

Decided by the maintainer on 2026-11-22 and 2026-11-23:

- **urzua is native**, reached without an adapter, and every other tool gets an adapter.
- **The contract is urzua's existing shape**, kept in agent-cli-kit, and urzua conforms by addition
  only.
- **The examined count** is `examined`, an object with one count per unit.

## Open questions

1. **The cost of counting for tsc.** `--listFilesOnly` is a second run of the compiler. Is the count
   worth that at the push trigger, or is it taken only in CI?
2. **Tool versions.** Whether an adapter refuses a tool version it has no recording for, or runs and
   warns.
3. **Tasks a runner replays from its cache.** A cached task has an exit code and a log but did not
   run. Is its `examined` count the one from the run that was cached?

## Non-goals

- Changing what a tool checks. An adapter reports what the tool said.
- A shared code library for the contract. Each tool validates its own output against the schemas.
- Running a tool's fixer. A finding names the tool's fix command; garita does not run it.

## References

- RFC-1: the checks, scopes, budgets and the plugin protocol (its extended contract) this builds on.
- RFC-3: the systems a tool belongs to.
- agent-cli-kit RFC-1 and RFC-3: the contract a native tool meets.
- urzua RFC-3: the output contract the shared one starts from.
