# Release App and ruleset setup

Maintainer steps for the release pipeline in [RFC-4](../rfc/RFC-4-ci-and-releases.md). Nothing here is
a file in the repository: the App, the two secrets and the ruleset are settings only a maintainer can
make. Until the secrets are set, `prepare-release.yml` prints a notice and does nothing, so pushes to
`main` stay green and no release is prepared.

## Steps

1. Use a GitHub App with no webhook and these repository permissions only: Contents (read and write)
   and Pull requests (read and write). An App the organisation already owns for releases can be
   reused: one App can serve several repositories. Otherwise create one.
2. Install the App on this repository. An App that is reused is installed on each repository it
   releases, and on no others.
3. Generate a private key for the App and copy the App's Client ID (not the App ID: the workflow
   passes it as `client-id`). A reused App gets a new key for this repository's secret, so one
   repository's key can be rotated without touching another's.
4. Set two repository secrets: `RELEASE_APP_CLIENT_ID` (the Client ID) and `RELEASE_APP_PRIVATE_KEY`
   (the whole PEM file). Then delete the local copy of the key.
5. Create a ruleset that targets `main` only. It should:
   - require a pull request, with 0 required approvals (with more, a sole maintainer cannot merge);
   - require the status check `ci`, from GitHub Actions;
   - block force pushes and deletions;
   - have an empty bypass list.
6. Confirm that the `release` branch and `v*` tags are not covered by any rule. A rule covering
   `release` blocks the workflow's force-push of that branch, and a rule covering `v*` blocks the tag
   that `knope release` creates with `GITHUB_TOKEN` in `publish-release.yml`.
7. Confirm that the `no-changeset` label exists. It is already created; the release pull request is
   given it because it consumes changesets instead of adding one.

## How to check it works

The first push to `main` after the secrets are set runs `prepare release`. With no changeset waiting
it ends green with the notice "nothing to release". With a changeset waiting, it pushes the `release`
branch and opens a pull request titled `chore: prepare release <version>`, labelled `no-changeset`.
The pull request changes `rust/Cargo.toml`, `rust/Cargo.lock`, `CHANGELOG.md`, and removes the
consumed files under `.changeset/`. The `ci` check runs on it without anyone approving a workflow run.

Merging that pull request runs `publish release`: it re-runs the checks on the merged commit, tags
`v<version>`, creates the release with the changelog section as notes, builds both archives, and
attaches them. The release should end with exactly two assets, `garita-x86_64-unknown-linux-gnu.tar.gz`
and `garita-aarch64-apple-darwin.tar.gz`.

## What goes wrong

- Only one of the two secrets is set: the run fails with a message saying both must be set. A
  half-configured App is an error.
- A wrong Client ID or a malformed key: the run fails at the token step.
- The ruleset covering `release` or `v*`: the branch push or the tag is rejected.
- A publish run that fails after the release was created: use "Re-run failed jobs", not "Re-run all
  jobs". The tag and the release already exist, so creating them again fails before anything is
  built.
- A release with no binaries: the release is public as soon as it is created and its two archives
  are attached a few minutes later. If the build fails, the release stays without them until the
  failed jobs are re-run.
- The ruleset requiring approvals: a sole maintainer cannot merge the release pull request.

## Rotating the key

Generate a new private key in the App settings, store it in `RELEASE_APP_PRIVATE_KEY`, and delete
this repository's old key from the App. When the App serves other repositories, leave their keys.
