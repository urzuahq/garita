---
default: patch
---

The git hooks show only the summary in a terminal.

At a terminal the hooks no longer print the JSON verdict. When the hook's output is not a terminal it is still printed; git passes a commit hook's output on its error stream, so that is where an agent finds it. `garita hooks install` updates hooks installed by an earlier version, and `garita doctor` says when they are outdated. A formatter's "no files" message seen through a task runner is no longer read as a file.
