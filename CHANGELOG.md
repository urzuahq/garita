# Changelog

## 0.2.0 (2026-11-27)

### Breaking Changes

#### garita describes its commands, and its reports follow the agent CLI contract's run profile.

`garita describe` prints one JSON object naming each command, its profile, whether it writes, and the scopes it accepts. A finding about the run itself, such as a budget or a widened scope, no longer carries `check`; a `check` that is present names an entry in `checks`. A check may now be named `garita`.

#### garita prints nothing but its report.

The summary on stderr is gone; the report on stdout is the only output. A receipt that could not be written, read or pruned is now an `info` finding about the run instead of a line on stderr.

#### garita declares its inputs, gives each next command as arguments, and keeps a check's output out of its findings.

- `garita describe` declares each command's arguments, garita's environment variable, its configuration file, that no command reads standard input, and that garita has no log.
- Every report carries `next_action_argv`, the next action as a program and its arguments, null when `next_action` is null. A finding with a `fix` carries `fix_argv`.
- A `not-run` report carries `error_kind`: `usage`, `config`, `environment` or `internal`. A usage error, including one with no command or an unknown one, is a `not-run` report whose `error` is one sentence naming the argument, with no usage text; a panic in any command is one of kind `internal`, and nothing reaches stderr.
- A finding's `message` is one sentence. What the check's command printed is on its entry in `checks`: `output_tail`, the end of its output, for a check that failed, warned or could not run; `output`, the path of the full output; and `output_notable`, the lines picked out as the likely reason. Under `via: turbo` a task's lines in `output_notable` start with the task's name.
- `doctor` no longer suggests appending to `.gitignore` as its next action, since that takes a shell redirection.
- The hooks are format 3: when `garita` is not on PATH, a hook prints a `not-run` report, with `error_kinds`, as one JSON line on stdout and exits 2. Run `garita hooks install` once in each repository to get it; `garita doctor` reports a format 1 hook as outdated until then.

### Features

- garita conforms to agent CLI contract 1.1.0, the final release of 1.1, vendored at its tag.

#### `garita risk` prints a change's risk class alone.

It reads git and the config, runs no check and writes no receipt. It prints a report with `risk` set and no checks, choosing its base as `garita run --trigger push` does, or from `--base`. `required` lists the checks the `ci` trigger runs, with no status. It answers `pass` whenever it could match, including when the merge base declares no policy or its policy cannot be read, and `not-run` when there is no repository or no config. `garita describe` declares it safe.

#### A change is matched to one of the repository's risk classes, and the run report says which.

A `risk` section in `.garita/config.yaml` lists the classes, lowest first, and the paths each covers. `garita run` reads the policy from the merge base, so a change cannot lower its own class, and reports `risk`: the class, how it was set, the paths that set it, how many people must approve, and whether the checks that ran passed. `.garita/**` and `.github/workflows/**` always match the highest class. When the merge base's policy cannot be read, the class is `null`, a person must approve, and the run goes on. A report says `risk: null` when the merge base declares no policy. A class may not yet require a system above 0.

### Fixes

- A format 2 hook, written by a build between releases, is still garita's: `garita hooks install` upgrades it to format 3 without `--replace`, and `garita doctor` reports it as outdated until then.

#### A `not-run` report names every kind of reason in `error_kinds`.

`error_kinds` replaces `error_kind`: an array, never empty, each of `usage`, `config`, `environment` and `internal` at most once and in that order. A run stopped by a mistyped check name and an unsupported check reports `["usage", "config"]`, where it reported only `usage`.

#### A formatter's "no files" message is never read as a file.

Run through a task runner, oxfmt's "no files" message arrives on the same stream as its file list, where it was read as an unformatted file. It is now recognised on either stream, and a line that is a sentence is never taken for a path.

#### The README shows how to install a release.

It no longer says nothing is released, and gives the one command that installs the latest release with its build provenance checked.

#### Reports name paths relative to the repository.

A path inside the repository, or inside its git directory, is written relative to the repository root in `error` and in garita's own messages, such as `.garita/config.yaml` or `.git/hooks/pre-commit`; outside a repository the error says "the current directory" without its path, and a path outside stays absolute. A name given to `garita run` that is not a check no longer gets a `checks` entry or an output file; the report stays `not-run` with a `usage` error naming each unknown name.

## 0.1.1 (2026-11-23)

### Fixes

#### An install script that verifies what it installs.

One command downloads the release for your platform and checks its build provenance with `gh`, or a sha256 you pin, before it installs anything. See `docs/reference/install.md`.

#### Failures read clearly, and lint run through a task runner reports file, line and rule.

For a failing check, the summary shows the most telling line it can find. Lint run through a task runner is read from each package's ordinary lint output. A task that fails is named, and a task the runner keeps no log for is handled. A check that examined nothing is always reported, as an error when its tool also failed.

Checks run through a task runner run one at a time and cannot read each other's results. An output-format flag for oxlint given to the task runner itself, before `--`, is reported as a configuration error. A check can set `min_severity` to keep its tool's lower-severity findings out of the verdict; they are counted instead. A failing command's message keeps lines that look like errors from anywhere in its output, plus the last lines of both its error and normal output. When two checks fail, one suggested command re-runs both, and it passes `--files` only to checks that take files.

## 0.1.0

First version.
