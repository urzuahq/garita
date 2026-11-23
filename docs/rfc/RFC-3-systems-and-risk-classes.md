---
Stable-Id: 01M838QGHEGFDWX5C3D82Z5D6J
Status: Draft
Date: 2026-11-23
Author: beauwilliams
---
# 3 — Systems and risk classes

## Summary

RFC-1 gives every change the same gate. This RFC lets a repository ask for more where more is at
stake.

1. **Systems.** Every tool garita runs belongs to one of three systems, named for the kind of compute
   that answers. System 0 is fast deterministic compute: urzua, linters, type checkers, build tools,
   test runners. System 1 is a decision model. System 2 is LLM inference.
2. **Risk classes.** Before any change is made, the repository declares which parts of it carry which
   risk, and how far up the systems a change to each must go. Risk is the one axis the three systems
   sit on.

garita is the control plane for both. It answers no question about the change itself. It decides
what runs, where, within which budget, and which evidence counts.

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

## Proposal

### 1. The control plane

garita holds four things and nothing else:

- **what runs:** the checks, and the system each belongs to;
- **where:** the trigger and scope (RFC-1 §2);
- **within what:** the budgets (RFC-1 §4);
- **what counts:** the risk classes, and which receipts are accepted as evidence (RFC-1 §8).

Every answer about the change comes from a tool. garita's own decision is always deterministic: given
the same tool results, it returns the same verdict.

"Control plane" names garita and nothing else. The records, with urzua as their engine, are the
**governance plane**: people decide there, in records. garita enforces what the records and the risk
classes say on every change, and returns one verdict. Systems 0, 1 and 2 are the checks the control
plane runs. Risk is a separate axis: it says how far up the systems a change must go.

### 2. Systems

| System | Compute | Tools | Deterministic |
|---|---|---|---|
| 0 | Fast, deterministic | urzua, oxlint, oxfmt, tsc, cargo, test runners, any command with an exit code | Yes |
| 1 | A decision model | Tools built with decision-agent-kit | No |
| 2 | LLM inference | An LLM reviewer | No |

A check declares its system, and its adapter's `identity` or its native tool's `describe` states it (RFC-2).
System 0 is the default, so every RFC-1 check is a System 0 check without change.

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
    native: right-test         # a tool built with decision-agent-kit
    threshold: 0.8
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
  that judges a change is the one on the target branch, and so is the definition of every check a
  class requires: its whole entry, and the top-level keys it inherits. Otherwise a change could keep its
  class and replace the reviewer with a command that always passes. `.garita/config.yaml` is matched
  to the highest declared class by construction.
- **A class needs a check to satisfy it.** A class that requires a system for which no check is
  declared is a configuration error: `not-run`, exit 2.
- **`default` is `low`** unless the repository says otherwise, under RFC-1's opt-in principle.
- `evidence.accept` overrides RFC-1 §8's setting for the class, so critical work can refuse local
  receipts while low-risk work accepts them.
- Missing a class's requirement is a `risk` finding at `error`.

### 4. When a model's verdict blocks

RFC-1's principle is "deterministic by default". With risk classes that means:

- A System 1 or System 2 check **blocks only where the change's risk class requires its system**.
  Anywhere else it is advisory and its findings are capped at `warning`, as in RFC-1.
- garita's part stays deterministic. It checks that the verdict exists, that its pin matches, that it
  came from a model the configuration allows, and that it says pass. It never judges what the model
  said.
- **A verdict is asked once and then cited.** A model can answer differently on a second run, so a
  required verdict is written to a receipt and a later run reads the receipt instead of asking again.
  The gate's result for a given change is then stable, though it is not reproducible from scratch.
- **What a verdict is pinned to.** The tree hash, the model, the prompt version, the task it was
  judged against, the check's own definition and the risk policy in force. A change to any of them makes the verdict stale, and a
  stale verdict does not count. Without the task in the pin, an old "this is the right test" would
  still be cited after the task changed.
- **A decision model returns a score, not prose.** A System 1 check declares the `threshold` its
  score is judged against, and the verdict records both.
- **A class should require only a measured check.** A System 1 or System 2 check that blocks should
  state how often it misses and how often it speaks wrongly, measured on recorded changes.

### 5. Where each system runs, and where a class is enforced

| Trigger | Budget (RFC-1 §3's example) | Systems it runs |
|---|---|---|
| `commit` | 3s | 0 |
| `push` | 60s | 0, 1 |
| `ci` | 10m | 0, 1, 2 |
| `merge` | its own | 0, 1, 2 |

Earlier triggers run what fits their budgets and report the highest system reached as information, so
a commit is never blocked for lacking a System 2 verdict it could not afford.

**A class that requires System 1 or 2 is enforced only at `ci` or `merge`.** A local hook can be
skipped (`--no-verify`, an environment variable, an agent hook that times out), so a requirement
judged only at commit or push is not enforced at all. Two rules follow:

- A configuration whose class requires System 1 or 2 and that runs no check for it at `ci` or
  `merge` is a configuration error.
- CI only binds when the forge requires its result before merging. `garita doctor` reports a
  repository whose target branch does not require garita's status, and a merge queue whose CI does
  not run on the queue's merged result.

Budgets apply to every system. For System 2 the metric that matters is cost, which RFC-1 §4 already
allows: a budget is a limit on any metric a check reports.

### 6. garita's extended contract

These keys go beyond the base layer and the run profile of agent-cli-kit (RFC-1, RFC-3): `risk` on
the report, and `system` and `verdict` on a check's entry. They are garita's own extended contract,
not the shared contract, and they contradict nothing in it. They are not built. When a second tool
needs them they are proposed to agent-cli-kit.

```json
{
  "risk": { "class": "critical", "required": 2, "reached": 1, "met": false,
            "matched_by": "crates/signing/**" },
  "checks": [
    { "name": "lint", "system": 0, "status": "pass" },
    { "name": "right-test", "system": 1, "status": "pass",
      "verdict": { "score": 0.93, "threshold": 0.8, "model": "…", "tree": "…", "task": "…",
                   "receipt": "…" } },
    { "name": "test-review", "system": 2, "status": "skipped" }
  ]
}
```

`risk.reached` is the highest system for which every required check passed, counting up from 0.

### 7. Relation to RFC-1 and to urzua

RFC-1 carries the three statements this RFC depends on: the principle "Deterministic by default";
§9 item 4, where a model check is advisory except where a risk class requires it; and the non-goal,
which rules out model-judged blocking only outside a declared risk class.

- urzua does not score risk; urzua RFC-27 §2 places risk scoring outside its engine. garita owns risk.
- urzua is a System 0 native check (RFC-2) and, through RFC-1 §9's `gate:` locator, can cite a garita
  receipt as evidence. A receipt that records the system reached lets a record cite a System 2
  verdict.
- Whether a urzua record can itself declare the system its evidence needs is a urzua question, left
  to a urzua RFC if a real case asks for it.

## Phases

Everything here is Phase 2 of RFC-1, alongside the `gate:` locator: risk classes, System 1 and
System 2 checks, verdict receipts and the `risk` finding. Phases 0 and 1 run System 0 checks only.

## Decided so far

Decided by the maintainer on 2026-11-22:

- **Risk is garita's.**
- **garita is the control plane**, not a System 0 tool. urzua, linters and build tools are System 0.
- **The records are the governance plane**, with urzua as their engine (the maintainer, 2026-11-23).
  People decide there; garita enforces it. "Control plane" is used for garita only.
- **The systems:** 0 asks whether the test exists and is linked properly; 1 asks whether it tests the
  right thing, answered by tools built on decision models; 2 asks what the test does, answered by LLM
  inference.
- **A model's verdict blocks where a declared risk class requires it**, and garita checks
  deterministically that the verdict exists, is pinned and says pass.

## Open questions

1. **What the task is.** System 1 asks whether a test matches the task. Where does garita get the
   task: the urzua record the change cites, the PR description, or a declared input per check?
2. **Which models count.** How a class names the models allowed to give a required verdict.
3. **A person's override.** Whether a named person can stand in for a failed or unavailable model
   verdict, and how that is recorded so it is visible later.
4. **A check that answers differently.** A model check that flips between pass and fail on the same
   pin is flaky. Whether such a check is quarantined (still run, unable to block) with an owner and
   an expiry.
5. **Matching beyond paths.** Packages, records a change cites, labels. A label is set on the forge,
   so a local run cannot see it; is that acceptable for a class that is judged only in CI?
6. **Systems in urzua.** Should urzua's `embodiment.tiers` record the system that verified a claim,
   or does that stay a garita-only axis?

## Non-goals

- Computing risk from the diff with a model or a score. A class is declared and matched; it is
  never inferred.
- Judging whether a model's verdict is right. garita checks that it exists, what gave it and what
  it pins.
- Model-judged blocking outside a declared risk class.

## References

- RFC-1: the checks, triggers, budgets, receipts and the `gate:` locator this builds on.
- RFC-2: adapters and native tools, whose `identity` part and native `describe` state a tool's system.
- urzua RFC-27: the boundary that puts risk scoring outside urzua.
- urzua ADR-101: the principles inherited, including the content-scope ceiling: a check verifies presence and shape, never meaning.
