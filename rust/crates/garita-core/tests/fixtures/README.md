# Tool recordings

Real runs of the tools the adapters parse (oxlint, oxfmt, tsc, the TypeScript native compiler `tsgo`,
turbo), kept as test fixtures. RFC-2 §3 and §5 explain why: the facts an adapter depends on are not
documented, so each adapter is written against a recording.

## Layout

- `workspace/` is a small pnpm and turbo workspace (`app`, `lib`, `docs`). `workspace/variants/`
  holds the source files that `record.sh` swaps into a package for a case (a lint error, a type
  error, a badly formatted file, and so on); they are outside every `tsconfig.json`.
- `recordings/<tool>@<version>/<case>/` holds one run:
  - `cmd`: the working directory relative to the workspace on the first line, then the argv, one
    argument per line.
  - `stdout`, `stderr`, `exit_code`.
  - For turbo cases, also `summary.json` (the run summary from `--summarize`) and `logs/` (each
    package's `.turbo/*.log`).
- The directory name carries the version of the tool that produced the recording. A new tool version
  gets a new directory; the old one stays until nothing tests against it.

The workspace root is replaced by `<WORKSPACE>`, the directory holding `pnpm` by `<PNPM_BIN>`, and
the commit and branch turbo reports by `<SHA>` and `<BRANCH>`. Nothing else is normalised, so timings, turbo run ids and task order differ between runs.

## Regenerate

```sh
cd workspace && pnpm install --frozen-lockfile && cd ..
./record.sh
```

`record.sh` deletes `recordings/` and rewrites it, and removes its build output and turbo caches when
it ends. It sets `NO_COLOR` and unsets the variables that make oxlint change its default format when
it detects an agent.
