---
default: patch
---

The summary line for a failing check now shows the error that failed it, and lint run through a task runner reports each problem with its file and line. Checks run through a task runner name the failing task.

A failing command's message keeps lines that look like errors from anywhere in its output, plus the last lines of both its error and normal output. When two checks fail, one suggested command re-runs both.
