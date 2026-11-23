---
default: patch
---

A formatter's "no files" message is never read as a file.

Run through a task runner, oxfmt's "no files" message arrives on the same stream as its file list, where it was read as an unformatted file. It is now recognised on either stream, and a line that is a sentence is never taken for a path.
