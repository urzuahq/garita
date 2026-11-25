---
default: patch
---

A `not-run` report names every kind of reason in `error_kinds`.

`error_kinds` replaces `error_kind`: an array, never empty, each of `usage`, `config`, `environment` and `internal` at most once and in that order. A run stopped by a mistyped check name and an unsupported check reports `["usage", "config"]`, where it reported only `usage`. The hook a missing garita runs keeps its format until agent CLI contract 1.1 is final.
