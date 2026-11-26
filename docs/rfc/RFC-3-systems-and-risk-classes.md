---
Stable-Id: 01M838QGHEGFDWX5C3D82Z5D6J
Status: Accepted
Date: 2026-11-23
Accepted-on: 2026-11-26
Author: beauwilliams
---
# 3 — Systems and risk classes

> **Accepted, 2026-11-26.** The maintainer accepted this record as amended, in answer to "Accept
> RFC-3 as amended, in full or in part": "Accept".

## Summary

RFC-1 gives every change the same gate. This RFC lets a repository ask for more where more is at
stake.

1. **Systems.** Every tool garita runs belongs to one of three systems, named for the kind of compute
   that answers. System 0 is fast deterministic compute: urzua, linters, type checkers, build tools,
   test runners. System 1 is a decision model. System 2 is LLM inference.
2. **Risk classes.** Before any change is made, the repository declares its own classes, in order,
   which parts of it each covers, how far up the systems a change to each must go, and whether a
   person must approve it. garita ships no class names. Risk is the one axis the three systems sit on.

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
  classes:                       # lowest first
    - { name: routine,  approvals: 0 }
    - { name: core,     approvals: 1, requires: 1 }
    - { name: guarded,  approvals: 1, requires: 2, evidence: { accept: ci } }
  default: core                  # omitted: the highest class
  protected: ["deploy/**"]       # added to the built-in set below
  enforce: true                  # false: report the class, block nothing
  match:
    - { paths: ["docs/**", "**/*.md"], class: routine }
    - { paths: ["crates/core/**"], class: core }
    - { paths: ["crates/signing/**", "crates/auth/**"], class: guarded }

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

| Class in the example | System 0 | System 1 | System 2 | People to merge |
|---|---|---|---|---|
| routine | required | not run | not run | none |
| core | required | required | not run | 1 |
| guarded | required | required | required | 1, not the author |

- **The classes are the repository's.** garita has no built-in class names, as urzua has no built-in
  record types. `classes` is a list, lowest first; the order, not the name, says which class is
  higher. Names are unique and non-blank. A consumer cannot know the names ahead of time, so the
  report carries the matched class's properties as well as its name (§6).
- **A class's fields.** `requires` is the highest system a change of the class must pass, 0 when
  omitted. `approvals` is how many people must approve before it merges, 1 when omitted.
  `author_counts` says whether the change's author counts as one of them, `false` when omitted.
  `evidence` overrides RFC-1 §8's `evidence.accept` for the class, so critical work can refuse local
  receipts while low-risk work accepts them.
- **`requires` is cumulative.** A class that requires System 2 requires 0 and 1 as well. A higher
  system is not asked until the ones below it pass, so money is not spent on a change that fails lint.
- **Matching is deterministic.** A class is matched from the paths a change touches, and the highest
  class any touched path matches is the change's class. A renamed or deleted file matches on its old
  path as well as its new one, so moving a file out of a critical directory is a critical change.
- **An unmatched path gets `default`.** `default` names a declared class; when it is omitted, an
  unmatched path gets the highest class. A path no rule names is never the cheapest case by accident.
- **Protected paths are the highest class by construction.** `.garita/**` and the forge's CI
  workflow files (`.github/workflows/**`) match the highest declared class whatever `match` says,
  since a change to either can change the gate itself. `protected` adds paths to that set; nothing
  removes one.
- **One policy.** The `risk` section of `.garita/config.yaml` is the only policy. No file in a
  subdirectory can add to it or ask for less.
- **The policy is read from the merge base.** A change cannot lower its own class: the `risk` section
  that judges a change is the one on the target branch, and so is the definition of every check a
  class requires: its whole entry, and the top-level keys it inherits. Otherwise a change could keep its
  class and replace the reviewer with a command that always passes.
- **A broken policy on the target branch has a way back.** When the merge base's configuration
  cannot be read or is invalid, the change has no class: the report says so (§6), a person must
  approve it, and the run goes on with the change's own checks instead of answering `not-run` for
  that reason alone. The change that repairs the policy can then merge, with a person's approval.
- **A class needs a check to satisfy it.** A class that requires a system for which no check is
  declared is a configuration error: `not-run`, exit 2.
- **A required check that gives no answer fails.** A check that a class requires and that crashed
  or was killed at its timeout is a failure for that class, as if its `on_crash` were `fail`
  (RFC-1 §1). An error or a timeout never leaves a requirement met.
- **Observe before enforcing.** With `enforce: false` the class is matched and reported in full, and
  its requirements decide nothing: a missed requirement is a `risk` finding at `info`, and the report
  says the class was observed. Every receipt records the class matched, so a repository can measure
  how its classes fall on its first changes before it enforces them. `enforce` is `true` when omitted.
- Missing a class's requirement is a `risk` finding at `error`.

#### Who may merge

A class's `approvals` and `author_counts` say who may merge a change of that class; what an agent
may do follows from them and the verdict:

| Verdict | `approvals: 0` | `approvals` 1 or more |
|---|---|---|
| `pass` | an agent may merge | an agent may review; people merge |
| `fail` | blocked | blocked |

There is no separate field for what an agent may do, so it cannot disagree with `approvals`.

garita enforces what it can see. `garita merge` (RFC-1 §7) does not merge, with `merge.mode: auto` or
`admin`, a change whose class needs a person; it posts its status and leaves the merge to a person.
garita cannot see who approves on the forge or who presses merge there: branch protection and code
owners enforce that, and the report states the policy they enforce.

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
not the shared contract, and they contradict nothing in it. When a second tool needs them they are
proposed to agent-cli-kit.

```json
{
  "risk": { "class": "guarded", "source": "matched", "enforced": true,
            "requires": 2, "reached": 1, "met": false,
            "approvals": 1, "author_counts": false, "needs_person": true,
            "matched_by": [ { "path": "crates/signing/key.rs", "pattern": "crates/signing/**",
                              "class": "guarded" } ],
            "required": [ { "check": "lint", "system": 0, "status": "pass" },
                          { "check": "right-test", "system": 1, "status": "pass" },
                          { "check": "test-review", "system": 2, "status": "skipped" } ] },
  "checks": [
    { "name": "lint", "system": 0, "status": "pass" },
    { "name": "right-test", "system": 1, "status": "pass",
      "verdict": { "score": 0.93, "threshold": 0.8, "model": "…", "tree": "…", "task": "…",
                   "receipt": "…" } },
    { "name": "test-review", "system": 2, "status": "skipped" }
  ]
}
```

- `risk` is `null` when the configuration declares no `risk` section, so a consumer can tell "no
  policy" from "the lowest class".
- `source` says how the class was set: `matched` (a rule or a protected path matched),
  `default` (no touched path matched), or `base-unreadable` (the merge base's policy could not be
  read; `class` is then `null` and `needs_person` is `true`).
- `matched_by` lists every touched path that set the class, with the pattern and the class it
  matched; a protected path's pattern is the built-in or `protected` entry. It is empty for
  `default`.
- `needs_person` is `true` when `approvals` is 1 or more. A consumer that finds no `needs_person`
  assumes a person is needed.
- `required` lists the checks the class requires at this trigger and their status.
- `reached` is the highest system for which every required check passed, counting up from 0.
- `enforced` is `false` when `enforce: false` (§3).

**Reading the class alone.** `garita risk [--base <ref>]` matches the change against the merge
base's policy and prints the report with `risk` and no checks run. It reads git and the
configuration and writes nothing, so `describe` declares it safe. Its base ref is chosen as
`garita run`'s is (RFC-1 §2). It answers `pass` with the class, or `not-run` when there is no
repository or no configuration, never a class it could not match.

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

- **Now: match and report.** §3's classes, matching, default, protected paths, the way back from a
  broken policy and the merge properties; §6's `risk` key and `garita risk`. A class may declare
  `requires` above 0 only once a check of that system can be declared, so until then every class
  requires 0, and the class decides nothing but what the report says.
- **Phase 2 of RFC-1, alongside the `gate:` locator:** System 1 and System 2 checks, §4's verdicts
  and their receipts, §5's enforcement by trigger, and the `risk` finding at `error`.

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

Asked by the maintainer, as direction, not acceptance: risk classes should ship in garita soon, as
the one source of a change's class for whoever routes its review and merge. Their words, given in
another session and dated 2026-11-27 there: "I think classes shouldn't be fixed but set by the user
of garita through config just like urzua."

Decided by the maintainer on 2026-11-26, answering this record's proposed amendment ("1 yes, 2. Yes,
3 yes fine for now, 4 yes"):

- **The amendment is written and brought for acceptance.**
- **An omitted `default` gives an unmatched path the highest class.**
- **The built-in protected set is `.garita/**` and the CI workflow files**, "fine for now".
- **The class is read alone with its own command**, `garita risk`, not a flag on `run`.

## Open questions

1. **What the task is.** System 1 asks whether a test matches the task. Where does garita get the
   task: the urzua record the change cites, the PR description, or a declared input per check?
2. **Which models count.** How a class names the models allowed to give a required verdict.
3. **A person's override.** Whether a named person can stand in for a failed or unavailable model
   verdict, or dismiss a finding after a failed review, who the policy names for it, and how that is
   recorded so it is visible later.
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
- Verifying who approved a change on the forge. The report states the policy; the forge enforces it.
- Model-judged blocking outside a declared risk class.

## References

- RFC-1: the checks, triggers, budgets, receipts and the `gate:` locator this builds on.
- RFC-2: adapters and native tools, whose `identity` part and native `describe` state a tool's system.
- urzua RFC-27: the boundary that puts risk scoring outside urzua.
- urzua ADR-101: the principles inherited, including the content-scope ceiling: a check verifies presence and shape, never meaning.
