---
default: patch
---

For a failing check, the summary shows the most telling line it can find. Lint run through a task runner is read from each package's ordinary lint output and reports file, line and rule. A task that fails is named, and a task the runner keeps no log for is handled.

Checks run through a task runner run one at a time and cannot read each other's results. A parser's flag given to the task runner itself, before `--`, is reported as a configuration error. A failing command's message keeps lines that look like errors from anywhere in its output, plus the last lines of both its error and normal output. When two checks fail, one suggested command re-runs both.
