---
default: patch
---

Reports name paths relative to the repository.

A path inside the repository, or inside its git directory, is written relative to the repository root in `error` and in garita's own messages, such as `.garita/config.yaml` or `.git/hooks/pre-commit`; outside a repository the error says "the current directory" without its path, and a path outside stays absolute. A name given to `garita run` that is not a check no longer gets a `checks` entry or an output file; the report stays `not-run` with a `usage` error naming each unknown name.
