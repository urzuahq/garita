---
default: patch
---

Failures read clearly, and lint run through a task runner reports file, line and rule.

For a failing check, the summary shows the most telling line it can find. Lint run through a task runner is read from each package's ordinary lint output. A task that fails is named, and a task the runner keeps no log for is handled. A check that examined nothing is always reported, as an error when its tool also failed.

Checks run through a task runner run one at a time and cannot read each other's results. An output-format flag for oxlint given to the task runner itself, before `--`, is reported as a configuration error. A check can set `min_severity` to keep its tool's lower-severity findings out of the verdict; they are counted instead. A failing command's message keeps lines that look like errors from anywhere in its output, plus the last lines of both its error and normal output. When two checks fail, one suggested command re-runs both, and it passes `--files` only to checks that take files.
