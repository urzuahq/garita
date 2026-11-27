---
default: patch
---

`garita risk` prints a change's risk class alone.

It reads git and the config, runs no check and writes no receipt. It prints a report with `risk` set and no checks, choosing its base as `garita run --trigger push` does, or from `--base`. `required` lists the checks the `ci` trigger runs, with no status. It answers `pass` whenever it could match, including when the merge base declares no policy or its policy cannot be read, and `not-run` when there is no repository or no config. `garita describe` declares it safe.
