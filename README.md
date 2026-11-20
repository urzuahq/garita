# Garita

The deterministic gate for agent-authored change. Garita runs the checks a change must pass at the
narrowest scope that covers the risk, holds the gate itself to declared budgets, and returns one
machine-first verdict that agents, git hooks and CI branch on: a `status`, typed `findings` and a
`next_action`.

Garita is part of the urzuahq ecosystem: [urzua](https://github.com/urzuahq/urzua) checks the records
a change traces to, garita checks the change, and
[decision-agent-kit](https://github.com/urzuahq/decision-agent-kit) asks whether an agent's turn is
really done.

Nothing is built yet. The design is [RFC-1](docs/rfc/RFC-1-garita-a-machine-native-gate-for-agent-authored-change.md), in draft.
