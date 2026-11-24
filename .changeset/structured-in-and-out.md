---
default: major
---

garita declares its inputs, gives each next command as arguments, and keeps a check's output out of its findings.

- `garita describe` declares each command's arguments, garita's environment variable, its configuration file, that no command reads standard input, and that garita has no log.
- Every report carries `next_action_argv`, the next action as a program and its arguments, null when `next_action` is null. A finding with a `fix` carries `fix_argv`.
- A `not-run` report carries `error_kind`: `usage`, `config`, `environment` or `internal`. A usage error, including one with no command or an unknown one, is a `not-run` report whose `error` is one sentence naming the argument, with no usage text; a panic in any command is one of kind `internal`, and nothing reaches stderr.
- A finding's `message` is one sentence. What the check's command printed is on its entry in `checks`: `output_tail`, the end of its output, for a check that failed, warned or could not run; `output`, the path of the full output; and `output_notable`, the lines picked out as the likely reason. Under `via: turbo` a task's lines in `output_notable` start with the task's name.
- `doctor` no longer suggests appending to `.gitignore` as its next action, since that takes a shell redirection.
- The hooks are format 2: when `garita` is not on PATH, a hook prints a `not-run` report as one JSON line on stdout and exits 2. Run `garita hooks install` once in each repository to get it; `garita doctor` reports a format 1 hook as outdated until then.
