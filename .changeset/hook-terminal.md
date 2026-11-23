---
default: patch
---

The git hooks show only the summary in a terminal.

At a terminal the hooks no longer print the JSON verdict, which agents and pipes still receive. `garita hooks install` updates hooks installed by an earlier version, and `garita doctor` says when they are outdated. A formatter's "no files" message seen through a task runner is no longer read as a file.
