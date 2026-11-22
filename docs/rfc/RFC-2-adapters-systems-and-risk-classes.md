---
Stable-Id: 01M82HJ6S3BY3PXRGJW1NXH0XZ
Status: Draft
Date: 2026-11-22
Author: beauwilliams
---
# 2 — Adapters, systems and risk classes

## Summary

RFC-1 gives every check one verdict: pass, warn or fail. This RFC says what garita is in relation to
the tools it runs, and adds three things on top of RFC-1.

**Garita is the control plane.** It answers no question about the change itself. It decides what
runs, where, within which budget, and which evidence counts, and it returns one verdict.

1. **Systems.** Every tool garita runs belongs to one of three systems, named for the kind of compute
   that answers. System 0 is fast deterministic compute: urzua, linters, type checkers, build tools,
   test runners. System 1 is a decision model. System 2 is LLM inference.
2. **Risk classes.** Before any change is made, the repository declares which parts of it carry which
   risk, and how far up the systems a change to each must go. Risk is the one axis the three systems
   sit on.
3. **Adapters.** Every tool, in any system, reaches garita through an adapter that describes it,
   invokes it at a scope, parses its output and reports its metrics. A tool that already speaks
   garita's contract is **native** and needs no adapter. urzua is the first native tool.

## Motivation

**The same pass, whatever is at stake.** Under RFC-1 a typo fix in a README and a change to the code
that signs releases pass the same gate in the same way. A repository that wants more for the second
has to write that policy somewhere else, usually as prose a reviewer is meant to remember.

**"Tested" is three different questions, and each needs a different kind of compute.**

| System | The question, asked of a test | What answers it | Cost |
|---|---|---|---|
| 0 | Does the test exist, and is it linked properly to the work it claims to cover? | Deterministic compute | Milliseconds to seconds |
| 1 | Is it the right test: does it test what the task says? | A decision model | Seconds |
| 2 | What does the test actually do? | LLM inference | Minutes, and money |

Agent-authored change is routinely accepted on the first question when the risk called for the third.
The opposite failure costs as much: asking an LLM about a typo fix.

**Risk is decided too late, by whoever is looking.** Whether a change needed a deeper look is usually
argued after it lands. Declaring it ahead of time, per area of the repository, turns it into
configuration a gate can enforce and a reviewer can read. A low-risk change needs a System 0 pass and
nothing more. A critical one is not done until System 2 has answered.

**Every new tool costs a parser.** RFC-1 §6 lists the parsers garita ships, and running real hooks
needs input-side wrapping as well: one run per package directory, output that arrives through a task
runner, checks that take no files. These are one concern in two places. A tool that already returns
structured findings should need neither.

## Proposal

### 1. The control plane

Garita holds four things and nothing else:

- **what runs:** the checks, and the system each belongs to;
- **where:** the trigger and scope (RFC-1 §2);
- **within what:** the budgets (RFC-1 §4);
- **what counts:** the risk classes, and which receipts are accepted as evidence (RFC-1 §8).

Every answer about the change comes from a tool. Garita's own decision is always deterministic: given
the same tool results, it returns the same verdict.

### 2. Systems

| System | Compute | Tools | Deterministic |
|---|---|---|---|
| 0 | Fast, deterministic | urzua, oxlint, oxfmt, tsc, cargo, test runners, any command with an exit code | Yes |
| 1 | A decision model | decision-agent-kit, and tools built on decision models | No |
| 2 | LLM inference | An LLM reviewer | No |

A check declares its system, and its adapter's `describe` states it. System 0 is the default, so
every RFC-1 check is a System 0 check without change.

A slow deterministic check is still System 0. Running a test on the merge base to see it fail there is
deterministic, so it is a System 0 check with a larger budget, placed at a later trigger.

### 3. Risk classes

```yaml
risk:
  classes:
    low:      { requires: 0 }
    standard: { requires: 1 }
    critical: { requires: 2, evidence: { accept: ci } }
  default: low
  match:
    - { paths: ["docs/**", "**/*.md"], class: low }
    - { paths: ["crates/core/**"], class: standard }
    - { paths: ["crates/signing/**", "crates/auth/**", ".github/workflows/**"], class: critical }

checks:
  right-test:
    system: 1
    native: decision-agent-kit
    on: [push, ci]
  test-review:
    system: 2
    run: reviewer --diff {base}
    parser: verdict
    budget: { duration: 5m, cost: 0.50usd }
    on: [ci, merge]
```

| Risk class | System 0 | System 1 | System 2 |
|---|---|---|---|
| low | required | not run | not run |
| standard | required | required | not run |
| critical | required | required | required |

- **`requires` is cumulative.** A class that requires System 2 requires 0 and 1 as well. A higher
  system is not asked until the ones below it pass, so money is not spent on a change that fails lint.
- **Matching is deterministic.** A class is matched from the paths a change touches, and the highest
  class any touched path matches is the change's class. A renamed or deleted file matches on its old
  path as well as its new one, so moving a file out of a critical directory is a critical change.
- **The policy is read from the merge base.** A change cannot lower its own class: the `risk` section
  that judges a change is the one on the target branch. `.garita/config.yaml` is matched to the
  highest declared class by construction.
- **A class needs a check to satisfy it.** A class that requires a system for which no check is
  declared is a configuration error, exit 1.
- **`default` is `low`** unless the repository says otherwise, under RFC-1's opt-in principle.
- `evidence.accept` overrides RFC-1 §8's setting for the class, so critical work can refuse local
  receipts while low-risk work accepts them.
- Missing a class's requirement is a `risk` finding at `error`.

### 4. When a model's verdict blocks

RFC-1 said only a deterministic check can block. With risk classes that becomes:

- A System 1 or System 2 check **blocks only where the change's risk class requires its system**.
  Anywhere else it is advisory and its findings are capped at `warn`, as in RFC-1.
- Garita's part stays deterministic. It checks that the verdict exists, that it is pinned to this
  exact tree, that it came from a model the configuration allows, and that it says pass. It never
  judges what the model said.
- **A verdict is asked once and then cited.** A model can answer differently on a second run, so a
  required verdict is written to a receipt pinned to the tree hash, the model and the prompt version.
  A later run over the same tree reads the receipt and does not ask again. The gate's result for a
  given tree is then stable, though it is not reproducible from scratch.

### 5. Where each system runs

| Trigger | Budget (RFC-1 §3 defaults) | Systems it runs |
|---|---|---|
| `commit` | 3s | 0 |
| `push` | 60s | 0, 1 |
| `ci` | 10m | 0, 1, 2 |
| `merge` | its own | 0, 1, 2 |

A class's requirement is judged at the **last** trigger the configuration runs for it, normally `ci`
or `merge`. Earlier triggers run what fits their budgets and report the highest system reached as
information, so a commit is never blocked for lacking a System 2 verdict it could not afford. Since
`--no-verify` skips the local hooks, the last trigger is also the one a person cannot skip.

Budgets apply to every system. For System 2 the metric that matters is cost, which RFC-1 §4 already
allows: a budget is a limit on any metric a check reports.

### 6. Adapters

An adapter has four parts:

| Part | What it does |
|---|---|
| `describe` | The tool's name, its system, the scopes it accepts and the metrics it reports |
| `invoke` | Turns a trigger's scope into the tool's arguments |
| `parse` | Turns the tool's output into findings with file, line, rule and message, and reports what it examined |
| `metrics` | The numbers a budget can limit (RFC-1 §4): duration, files examined, tests run, cost |

RFC-1's parsers become the `parse` part of built-in adapters. A plugin adapter uses RFC-1 §6's
subprocess protocol, with `--describe` returning the `describe` part.

`invoke` owns everything about how a tool is called:

- **Placeholders.** `{files}` and `{affected}` are filled from the scope (RFC-1 §3). A check whose
  command has no placeholder always runs. That is not a `no-input` skip.
- **`partition`.** One run per matching root, from inside it, with paths rebased to the root.
  Findings are mapped back to repository-relative paths.
- **`exclude`.** Globs removed from the check's input.
- **`via`.** The tool runs through a task runner, for example `via: turbo`. The adapter splits the
  runner's output by task, maps each task to its package directory and hands each part to `parse`.

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
```

### 7. Native tools

A tool is **native** when it speaks garita's contract itself, so it needs no adapter:

- stdout is one JSON object with a `status` and typed findings, each with file, line, rule and message;
- it reports what it examined, so a run over nothing is distinguishable from a pass;
- it returns a `next_action`, the narrowest command to run next;
- it accepts a scope as arguments (paths, a base ref);
- it answers `describe` with the same shape a plugin adapter returns, including its system.

A native check is declared with `native: <tool>` instead of `run` and `parser`. Garita passes it the
scope and reads its result unchanged.

**urzua is the first native tool**, in System 0. RFC-1 already defines garita's output as the urzua
contract (urzua RFC-3). `urzua check` returns one JSON object, `status`, `findings[]`, a
files-examined count and `next_action`, and takes paths and `--base`. What it lacks, for this RFC, is
a `describe` answer, and agreement on the key names it shares with garita: urzua reports
`files_examined`, RFC-1 §5 writes `examined`.

decision-agent-kit is the intended first System 1 tool.

### 8. Contract additions

```json
{
  "risk": { "class": "critical", "required": 2, "reached": 1, "met": false,
            "matched_by": "crates/signing/**" },
  "checks": [
    { "name": "lint", "system": 0, "status": "pass" },
    { "name": "right-test", "system": 1, "status": "pass",
      "verdict": { "model": "…", "tree": "…", "receipt": "…" } },
    { "name": "test-review", "system": 2, "status": "skipped" }
  ]
}
```

`risk.reached` is the highest system for which every required check passed, counting up from 0.

### 9. Relation to RFC-1

This RFC changes three statements in RFC-1, in the same change:

- the principle "Deterministic blocks, advisory everything else" becomes "Deterministic by default"
  (section 4 above);
- §9 item 4, model checks being advisory, gains the risk-class exception;
- the non-goal "Model-judged blocking" is narrowed to blocking outside a declared risk class.

### 10. Relation to urzua

- urzua does not score risk; urzua RFC-27 §2 places risk scoring outside its engine. Garita owns risk.
- urzua is a System 0 native check and, through RFC-1 §9's `gate:` locator, can cite a garita receipt
  as evidence. A receipt that records the system reached lets a record cite a System 2 verdict.
- Whether a urzua record can itself declare the system its evidence needs is a urzua question, left
  to a urzua RFC if a real case asks for it.

## Phases

- **Phase 0:** the adapter interface with `partition`, `exclude` and `via`, used by the Phase 0
  parsers. Every check is System 0. No native protocol, no risk.
- **Phase 1:** native tools, `describe`, and urzua as the first native check; the shared contract's
  key names settled.
- **Phase 2:** risk classes, System 1 and System 2 checks, verdict receipts and the `risk` finding,
  alongside the `gate:` locator.

## Decided so far

Decided by the maintainer on 2026-11-22:

- **Risk is garita's.**
- **Garita is the control plane**, not a System 0 tool. urzua, linters and build tools are System 0.
- **urzua is the first native tool**, reached without an adapter.
- **The systems:** 0 asks whether the test exists and is linked properly; 1 asks whether it tests the
  right thing, answered by tools built on decision models; 2 asks what the test does, answered by LLM
  inference.
- **A model's verdict blocks where a declared risk class requires it**, and garita checks
  deterministically that the verdict exists, is pinned to the tree and says pass.

## Open questions

1. **One contract.** Is the shared contract a document in urzua (RFC-3), in garita, or in its own
   repository, and how are its versions negotiated?
2. **The examined key.** `files_examined` as urzua ships it, or `examined` as RFC-1 §5 writes it.
   Phase 0 emits this key, so it is the one question here that cannot wait.
3. **What the task is.** System 1 asks whether a test matches the task. Where does garita get the
   task: the urzua record the change cites, the PR description, or a declared input per check?
4. **What pins a verdict.** Tree hash, model and prompt version are proposed. Does a change to the
   task, or to the risk policy, invalidate a cached verdict?
5. **Which models count.** How a class names the models allowed to give a required verdict, and
   whether a person can stand in for one.
6. **Matching beyond paths.** Packages, records a change cites, labels. A label is set on the forge,
   so a local run cannot see it; is that acceptable for a class that is judged only in CI?
7. **Systems in urzua.** Should urzua's `embodiment.tiers` record the system that verified a claim,
   or does that stay a garita-only axis?

## Non-goals

- Computing risk from the diff with a model or a score. A class is declared and matched; it is
  never inferred.
- Judging whether a model's verdict is right. Garita checks that it exists, what gave it and what
  it pins.
- Model-judged blocking outside a declared risk class.

## References

- RFC-1: the checks, triggers, budgets, parsers, receipts and the `gate:` locator this builds on.
- urzua RFC-3: the output contract a native tool speaks.
- urzua RFC-27: the boundary that puts risk scoring outside urzua.
- urzua ADR-101: the principles inherited, including the content-scope ceiling: a check verifies presence and shape, never meaning.
