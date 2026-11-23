---
default: patch
---

The summary line for a failing check now shows the error that failed it, and lint run through a task runner reports each problem with its file and line.

A failing command's message keeps the last lines of both its error and normal output, so the line that explains the failure is no longer cut. When two checks fail, one suggested command re-runs both.
