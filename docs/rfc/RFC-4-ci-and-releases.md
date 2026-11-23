---
Stable-Id: 01M848BPD59AS52DMCTTXMHE6T
Status: Draft
Date: 2026-11-23
Author: beauwilliams
---
# 4 — CI and releases

## Summary

How garita is checked and how it is released. One required check gates every change. Every platform
garita is released for is tested, not only built. A release is prepared by a workflow and published
by a person merging one pull request. Binaries are built in CI with provenance attached, and an
install script puts a verified binary on a machine.

## Motivation

**The hooks call `garita` by name.** `garita hooks install` writes hooks that run `garita run`, and a
hook fails when garita is not on the machine (RFC-1 §7). So every machine and every checkout that
commits to a gated repository needs the same garita, installed the same way. A binary someone built
by hand on one machine does not give that.

**A platform that is only built has not been tested.** Garita stops a timed-out tool's whole process
group by running the system `kill`, because the workspace forbids `unsafe`, and it reads the machine
name differently on each platform. Until this RFC neither had run anywhere but Linux, while the
gate is meant to run on developers' Macs.

**A release should be a decision, and a visible one.** A tag pushed from a laptop publishes whatever
that laptop had. A release that is a pull request can be read, checked and merged like any change.

**A repository's required check has to have one name.** Jobs are added and split over time. Branch
protection that lists each of them goes stale; one job that aggregates the rest does not.

## Proposal

### 1. One required check

Every pull request, merge-queue entry and push to `main` runs `ci.yml`. Its last job is named `ci`.
It needs every other job and fails unless each succeeded; a job that was skipped, cancelled or never
reported counts as failed, except a job that is meant to be skipped off pull requests. `ci` is the
only check branch protection requires.

A commit must end with one verdict. Two runs can start for the same commit: the release tool pushes
the release branch and then labels its pull request, and a person can label a pull request while its
checks run. If the later run cancelled the earlier one, the cancelled run's `ci` job would still
report, as a failure, and could be the last word on a commit whose other run is green; the ruleset
then refuses a merge that should be allowed. A gate job cannot abstain: any conclusion it gives for
a superseded run is either a false block or a pass before the real answer. So runs are not
cancelled on the same commit. Only a new commit cancels a run in progress, and that failure lands
on the old commit. Any other event waits behind the run in progress, which makes the run for the
latest event the last to report. Editing a pull request's title or description starts no run.

The checks themselves live in a reusable workflow, `checks.yml`, so that publishing a release can
run exactly the same checks on the commit it ships (section 4): format, lint, unused dependencies
and dependency policy, build, tests, and the contract check that validates garita's output against
the vendored contract.

### 2. Every released platform is tested

The test job runs on Linux and on macOS. A platform is added to the release build only together
with a test job for it.

Tests run under a guard that fails when a test leaves anything behind in the temporary directory.
The suite is not split across jobs while it runs in seconds; it is split when one run takes longer
than about a minute.

### 3. Changesets are the only release trigger

A pull request that changes what an installer receives (`rust/`, `README.md`, `LICENSE`) adds a file
under `.changeset/` that says `patch`, `minor` or `major` and describes the change for a person
installing garita. The `changeset` job fails without one. A pull request that changes nothing an
installer receives needs none, and one that does can opt out with the `no-changeset` label.

Commit messages never decide a version. A change described as breaking must declare `major`.

### 4. A release is a pull request, and merging it publishes

- On each push to `main`, a workflow gathers the pending changesets, bumps the version, writes the
  changelog and opens or updates one pull request from the `release` branch. It uses a GitHub App's
  token, so that CI runs on that pull request; a pull request opened with the workflow's own token
  would start no checks.
- A person merges the release pull request. That is the release decision.
- Merging it runs the publish workflow, which first runs `checks.yml` on the merged commit. It
  verifies the tree it ships and does not ask whether some earlier run was green. It then tags the
  version, creates the release with the changelog as its notes, builds the binaries and attaches
  them.

### 5. What is published

- **Binaries on a GitHub release**, for Linux x86_64 and macOS on Apple Silicon, built in CI from the
  tagged commit with `--locked` and the pinned toolchain. Each archive carries a build provenance
  attestation.
- **An install script** that picks the platform, downloads the archive over HTTPS, verifies it by its
  attestation or by a checksum the caller gives, refuses anything it cannot verify unless told to
  skip verification, checks that the binary reports the version asked for, and only then puts it in
  place.

RFC-1 §11 also names crates.io and an npm wrapper. Those are not part of this RFC: whether tools
are published there is being decided for the whole organisation, and garita follows that decision.

### 6. Pins

Every third-party action is pinned by commit hash. Tools CI downloads (the test runner, the release
tool, the dependency checkers) are pinned by version and verified by checksum, each in one place; a
test fails when a workflow carries its own copy of a pin.

## What needs setting up outside a pull request

- A GitHub App with read and write access to contents and pull requests, installed on this
  repository, and its client id and private key as the secrets `RELEASE_APP_CLIENT_ID` and
  `RELEASE_APP_PRIVATE_KEY`.
- A ruleset on `main`: a pull request is required, the `ci` check is required, and force pushes and
  deletion are not allowed. It does not cover the `release` branch or version tags.
- The `no-changeset` label.

## Phases

1. The CI shape: `checks.yml`, the `ci` gate, tests on macOS, the changeset gate, the release
   invariants and the release tool's configuration.
2. The prepare and publish workflows. They do nothing until the App, the secrets and the ruleset
   exist.
3. The install script and its test.

## Open questions

1. **More platforms.** Linux on ARM and macOS on Intel are named in RFC-1 §11. Each needs a runner to
   test on before it is released.
2. **When to split the tests.** A minute is a guess; the measure should be how long a contributor
   waits for `ci`.

## Non-goals

- Publishing to a package registry.
- A release cadence. A release happens when its pull request is merged.

## References

- RFC-1: the hooks that call garita by name (§7) and its packaging (§11).
