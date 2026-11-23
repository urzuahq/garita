# Garita

The control plane that gates agent-authored change. Garita runs the tools that check a change at the
narrowest scope that covers the risk, holds the gate itself to declared budgets, and returns one
machine-first verdict that agents, git hooks and CI branch on: a `status`, typed `findings` and a
`next_action`.

Garita is part of the urzuahq ecosystem. The records, with
[urzua](https://github.com/urzuahq/urzua) as their engine, are the governance plane: people decide
there. Garita is the control plane: it enforces what the records and the declared risk say on every
change. [decision-agent-kit](https://github.com/urzuahq/decision-agent-kit) is the kit for the
decision-model checks garita can run, such as whether an agent's turn is really done.

## Status

In development: the RFCs are in draft and there is no release yet.

- `garita run` works: it reads `.garita/config.yaml`, resolves the scope from git, runs the checks
  with no shell, applies budgets, writes a receipt and prints one JSON verdict. It exits 0 when
  nothing blocks, 1 when something does, and 2 when it could not run.
- Built-in adapters read oxlint, oxfmt and tsc output, and any other command through its exit code.
  A task run through turbo is read from turbo's own run summary.
- `garita hooks install` writes the `pre-commit` and `pre-push` hooks into the directory git uses,
  and never overwrites a hook it did not write. `garita doctor` says whether the gate will run on
  this machine.

To try it, build from source with Rust 1.87 or later:

```sh
cd rust && cargo build --release
```

## Design

- [RFC-1](docs/rfc/RFC-1-garita-a-machine-native-gate-for-agent-authored-change.md): the gate, its
  configuration, budgets, output and receipts.
- [RFC-2](docs/rfc/RFC-2-adapters-and-native-tools.md): adapters and native tools.
- [RFC-3](docs/rfc/RFC-3-systems-and-risk-classes.md): systems and risk classes, a later phase.

Garita's output follows the contract in
[agent-cli-kit](https://github.com/urzuahq/agent-cli-kit).
