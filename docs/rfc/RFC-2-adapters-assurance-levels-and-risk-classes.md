---
Stable-Id: 01M82HJ6S3BY3PXRGJW1NXH0XZ
Status: Draft
Date: 2026-11-22
Author: beauwilliams
---
# 2 — Adapters, assurance levels and risk classes

## Summary

RFC-1 gives every check one verdict: pass, warn or fail. This RFC adds three things on top of it.

1. **Adapters.** Every tool reaches garita through an adapter that describes it, invokes it at a
   scope, parses its output and reports its metrics. A tool that already speaks garita's contract is
   **native** and needs no adapter. urzua is the first native tool.
2. **Assurance levels.** A pass can mean different things. Level 0: the evidence exists and is linked.
   Level 1: someone attested that it is the right evidence for what was asked. Level 2: it was
   exercised, and shown to fail without the change. Each check reports the level its pass reached.
3. **Risk classes.** Before any change is made, the repository declares which parts of it carry which
   risk, and the level each class requires. The gate is met when the change reaches the level its class
   requires, judged at the last trigger the class names.

The adapters and the policy above them are garita's control plane: what runs, where, at what level,
and which evidence counts.

## Motivation

**The same pass, whatever is at stake.** Under RFC-1 a typo fix in a README and a change to the code
that signs releases pass the same gate in the same way. A repository that wants more for the second
has to write that policy somewhere else, usually as prose a reviewer is meant to remember.

**"Tested" is three different claims.** Agent-authored change is routinely accepted on the first of
these when the risk called for the third:

| Level | The question | Who can answer it |
|---|---|---|
| 0, linked | Does a test exist for this, and is it linked to the work it claims to cover? | A machine, mechanically |
| 1, attested | Is it the right test: does it test what the task says? | A person or a named reviewer, by judgment |
| 2, exercised | What does it actually do: does it fail without the change and pass with it? | A machine, by running it on both trees |

urzua answers level 0 for records today: a `Realized-by` locator resolves, its pin matches, its tier
is `Verified`. urzua's own working rules ask for level 2 ("a planted-violation test, observed failing
before the fix, then passing after") and for level 1 in review, but both are recorded as prose that
nothing checks. The same gap exists in every repository garita gates.

**Risk is decided too late, by whoever is looking.** Whether a change needed a deeper check is
usually argued after it lands. Declaring it ahead of time, per area of the repository, turns it into
configuration a gate can enforce and a reviewer can read.

**Every new tool costs a parser.** RFC-1 §6 lists the parsers garita ships, and its amendments add
input-side wrapping (`partition`, `via: turbo`, scope placeholders). These are the same concern, in
two places. A tool that already returns structured findings should not need either.

## Proposal

### 1. Adapters

An adapter has four parts:

| Part | What it does |
|---|---|
| `describe` | The tool's name, the scopes it accepts, the metrics it reports and the highest assurance level it can produce |
| `invoke` | Turns a trigger's scope into the tool's arguments: `{files}`, `{affected}`, `partition`, `via` |
| `parse` | Turns the tool's output into findings with file, line, rule and message, and reports what it examined |
| `metrics` | The numbers a budget can limit (RFC-1 §4): duration, files examined, tests run |

RFC-1's parsers become the `parse` part of built-in adapters. A plugin adapter uses RFC-1 §6's
subprocess protocol, with `--describe` returning the `describe` part.

### 2. Native tools

A tool is **native** when it speaks garita's contract itself, so it needs no adapter:

- stdout is one JSON object with a `status` and typed findings, each with file, line, rule and message;
- it reports what it examined, so a run over nothing is distinguishable from a pass;
- it returns a `next_action`, the narrowest command to run next;
- it accepts a scope as arguments (paths, a base ref);
- it answers `describe` with the same shape a plugin adapter returns.

A native check is declared with `native: <tool>` instead of `run` and `parser`. Garita passes it the
scope and reads its result unchanged.

**urzua is the first native tool.** RFC-1 already defines garita's output as the urzua contract (urzua
RFC-3). `urzua check` returns one JSON object, `status`, `findings[]`, a files-examined count and
`next_action`, and takes paths and `--base`. What it lacks, for this RFC, is a `describe` answer, and
agreement on the key names it shares with garita: urzua reports `files_examined`, RFC-1 §5 writes
`examined`. One contract, defined once and versioned, is the condition for "native" to mean anything.

### 3. Assurance levels

Each check's result carries the level its pass reached, and the evidence for it:

- **Level 0, linked.** The check passed and covered what it claims. For urzua, the record's evidence
  locators resolve and their pins match. For a test runner, the tests that cover the changed files ran
  and passed.
- **Level 1, attested.** A level-0 pass, plus an attestation: who checked that the evidence is the
  right evidence for the task, against which pinned version of it, and when. Garita checks that the
  attestation exists, names an allowed attester and matches the pin. It never judges what the
  attestation says.
- **Level 2, exercised.** A level-0 pass, plus a run of the same tests on the merge base that shows
  them failing there, or a mutation score at or above a declared threshold. This needs a second
  worktree at the merge base, so it runs only at triggers that can afford it.

A model can produce an attestation, but under RFC-1's principle that only deterministic checks block, a
model attestation counts toward level 1 only where the repository's configuration says it does.

### 4. Risk classes

```yaml
risk:
  classes:
    low:      { requires: 0 }
    standard: { requires: 1, attesters: [maintainers] }
    critical: { requires: 2, attesters: [maintainers], evidence: { accept: ci } }
  default: standard
  match:
    - { paths: ["docs/**", "**/*.md"], class: low }
    - { paths: ["crates/signing/**", "crates/auth/**", ".github/workflows/**"], class: critical }
```

- A class is matched deterministically from what the change touches: paths first, and later packages
  or the records a change cites. The highest class any touched file matches is the change's class.
- `requires` is the level every relevant check must reach. `attesters` names who may attest for
  level 1. `evidence.accept` overrides RFC-1 §8's setting for the class, so critical work can refuse
  local receipts while low-risk work accepts them.
- The result names the class, the required level and whether it was met. Missing it is a `risk`
  finding at `error`.

### 5. Where each level runs

| Trigger | Budget (RFC-1 §3 defaults) | Levels it can produce |
|---|---|---|
| `commit` | 3s | 0 |
| `push` | 60s | 0, and 1 when an attestation is already present |
| `ci` | 10m | 0, 1, 2 |
| `merge` | its own | 0, 1, 2 |

A class's requirement is judged at the **last** trigger the configuration runs for it, normally `ci`
or `merge`. Earlier triggers run what fits their budgets and report the level they reached as
information, so a commit is never blocked for lacking a level-2 run it could not afford.
`on_exceed: demote` (RFC-1 §4) already moves slow work later; level-2 checks are demoted by
construction. Since `--no-verify` skips the local hooks, the last trigger is also the one a person
cannot skip.

### 6. Contract additions

```json
{
  "risk": { "class": "critical", "required": 2, "reached": 1, "met": false,
            "matched_by": "crates/signing/**" },
  "checks": [
    { "name": "test", "status": "pass", "level": 1,
      "evidence": { "attestation": "…", "base_run": null } }
  ]
}
```

`level` is per check; `risk.reached` is the lowest level among the checks the class requires.

### 7. Relation to urzua

- urzua does not score risk; urzua RFC-27 §2 places risk scoring outside its engine. Garita owns risk.
- urzua is a native check (section 2) and, through RFC-1 §9's `gate:` locator, can cite a garita
  receipt as evidence. A receipt that records `level` lets a record cite a level-2 run as its
  evidence.
- Whether a urzua record can itself declare the level its evidence needs is a urzua question, left
  to a urzua RFC if a real case asks for it.

## Phases

- **Phase 0:** the adapter interface, used by the Phase 0 parsers, with only the parts they need. No
  native protocol, no levels, no risk.
- **Phase 1:** native tools, `describe`, and urzua as the first native check (RFC-1 puts the urzua
  parser in Phase 1); the shared contract's key names settled.
- **Phase 2:** assurance levels, risk classes and the `risk` finding, alongside the `gate:` locator
  they depend on.

## Decided so far

- **Risk is garita's** (the maintainer, 2026-11-22: "Yeah risk is for garita").
- **urzua is native** (the maintainer, 2026-11-22: "urzua is the first garita native tool for its
  control plane ("adapter-less")").

## Open questions

1. **One contract.** Is the shared contract a document in urzua (RFC-3), in garita, or in its own
   repository, and how are its versions negotiated?
2. **Where an attestation lives.** A urzua record, a receipt, a signed commit trailer, or a review on
   the forge? Each has different strength and portability.
3. **Model attestations.** Can a model's attestation ever count toward level 1, and if so, only with a
   person co-signing?
4. **What counts for level 2.** Red/green on the merge base only, or mutation testing as an
   alternative, with what threshold?
5. **Matching beyond paths.** Packages, records a change cites, labels. A label is set on the forge, so
   a local run cannot see it; is that acceptable for a class that is judged only in CI?
6. **Levels as urzua tiers.** Should urzua's `embodiment.tiers` gain the levels (`Verified` split into
   linked, attested and exercised), or do they stay a garita-only axis?

## Non-goals

- Computing risk from the diff with a model or a score. A class is declared and matched; it is
  never inferred.
- Judging whether an attestation is right. Garita checks that it exists, who made it and what it
  pins.
- Model-judged blocking, unchanged from RFC-1.

## References

- RFC-1: the checks, triggers, budgets, parsers, receipts and the `gate:` locator this builds on.
- urzua RFC-3: the output contract a native tool speaks.
- urzua RFC-27: the boundary that puts risk scoring outside urzua.
- urzua ADR-101: the principles inherited, including the content-scope ceiling: a check verifies presence and shape, never meaning.
