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
| `describe` | The tool's name, its system (RFC-3), the kind of check it is, the scopes it accepts and the metrics it reports |
| `invoke` | Turns a trigger's scope into the tool's arguments and working directory |
| `parse` | Turns the tool's output into findings, reports what it examined, and classifies the run |
| `metrics` | The numbers a budget can limit (RFC-1 §4): duration, files examined, tests run |

A built-in adapter is code in garita. A plugin adapter is a subprocess that speaks RFC-1 §6's
protocol, with `--describe` returning the `describe` part.

### 2. Invoke

`invoke` owns everything about how a tool is called:

- **Placeholders.** `{files}` and `{affected}` are filled from the scope (RFC-1 §3). A check whose
  command has no placeholder always runs. That is not a `no-input` skip.
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
  `fix` command when it has one, and a `fingerprint` that is stable across runs. These are the fields
  the contract's gate profile names, and the ones SARIF and code-quality reports need.
- **What was examined.** A count per unit (`files`, `tests`, `packages`), taken from the tool's own
  output where it reports one. This is how a run over nothing is told apart from a pass.
- **The kind of run.** One of: clean, findings, ran on nothing, or crashed. The adapter decides this
  from the output and never from the exit code alone. A non-zero exit with no parseable output is a
  crash (RFC-1's `on_crash`).

What the first adapters read:

| Tool | Source | Examined count | Limits |
|---|---|---|---|
| oxlint | `--format json` | The report's count of files linted | Exit 1 covers findings, no files matched and a bad config alike |
| oxfmt | `--list-different` | The files passed to it | Paths only, so a finding has a file and no line or rule |
| tsc | `--pretty false` diagnostic lines | To be settled from a recorded run | No structured output; multi-line messages continue on indented lines |
| `exit-code` | The exit code and the last lines of output | The files passed to it | The finding's message is the output tail |

**Each adapter is written against a recorded run of the real tool,** and the recordings are its test
fixtures, with the tool's version in the fixture's name. Documentation is not enough: the facts an
adapter depends on (exit codes, the text for no files matched, whether paths are relative to the
working directory) are mostly undocumented.

### 4. Metrics

Duration is measured by garita for every check. An adapter can report others, and a budget can name
any of them (RFC-1 §4). The examined counts are metrics too, so a check that suddenly examines far
fewer files than its history is visible the same way a slow one is.

### 5. Tools behind a task runner

With `via`, the command garita runs is the runner's, and the runner runs the tool once per package.
The adapter needs, per task: the package directory, the exit code and that task's output, which it
hands to `parse` with paths rebased from the package directory.

The preferred source is the runner's own structured record of the run (for turbo, its run summary
and per-task logs), because prefixed and interleaved terminal output is the least stable thing a
runner prints. Splitting prefixed output with grouped log order is the fallback. Which of the two
turbo supports well enough is settled from a recorded run before this adapter is written.

A runner returns the highest exit code among its tasks, so it cannot tell findings from a crash. The
adapter classifies each task separately.

### 6. Native tools

A tool is **native** when it meets the agent CLI contract (agent-cli-kit RFC-1): one JSON object on
stdout, exit codes 0, 1 and 2, a `status`, `blocking`, typed `findings`, an `examined` count, a
`next_action`, and a `describe` command.

- A native check is declared with `native: <tool>` in place of `run` and `parser`. Garita passes it
  the scope and reads its report unchanged.
- **Version check.** Garita reads the tool's `describe` once per run and compares the major of its
  `contract_version` with its own. A different major makes the check `not-run`, with a finding that
  names both versions. Garita never guesses at a report it does not understand. Unknown keys from a
  newer minor are ignored.
- **A native tool that exits 2 could not run.** The check is `not-run`, the same as a tool that
  cannot be started, and so is the garita run (RFC-1 §5). It is not a crash.
- **urzua is the first native tool.** The contract is written from its report: the exit codes,
  `status`, `blocking`, `findings`, and `next_action` with its reason and inputs, each input an
  object naming the placeholder, its flag and what to put there. What urzua lacks is the `describe`
  command and the `examined` object, which it adds beside its existing `files_examined` in a minor
  release. Nothing in its output changes or is removed.

Garita is itself a native tool: its own output meets the contract, so one garita can run another, and
urzua or decision-agent-kit can read a garita report without an adapter.

## Phases

- **Phase 0:** the adapter interface with `partition`, `exclude` and `via`; adapters for oxlint,
  oxfmt, tsc and `exit-code`. Its first step is recording one real run of each tool.
- **Phase 1:** native tools, the version check, urzua as the first native check, and plugin
  adapters; adapters for Vitest and cargo.

## Decided so far

Decided by the maintainer on 2026-11-22 and 2026-11-23:

- **urzua is native**, reached without an adapter, and every other tool gets an adapter.
- **The contract is urzua's existing shape**, kept in agent-cli-kit, and urzua conforms by addition
  only.
- **The examined count** is `examined`, an object with one count per unit.

## Open questions

1. **Turbo's structured output.** Whether its run summary and task logs carry enough (exit code and
   output per task) to avoid parsing prefixed output.
2. **The examined count for tsc.** Whether the file count comes from `--listFiles`, from extended
   diagnostics, or cannot be had from the TypeScript 7 compiler.
3. **Tool versions.** Whether an adapter refuses a tool version it has no recording for, or runs and
   warns.

## Non-goals

- Changing what a tool checks. An adapter reports what the tool said.
- A shared code library for the contract. Each tool validates its own output against the schemas.
- Running a tool's fixer. A finding names the tool's fix command; garita does not run it.

## References

- RFC-1: the checks, scopes, budgets and the plugin protocol this builds on.
- RFC-3: the systems a tool belongs to.
- agent-cli-kit RFC-1: the contract a native tool meets.
- urzua RFC-3: the output contract the shared one starts from.
