---
default: major
---

garita describes its commands, and its reports follow the agent CLI contract's run profile.

`garita describe` prints one JSON object naming each command, its profile, whether it writes, and the scopes it accepts. A finding about the run itself, such as a budget or a widened scope, no longer carries `check`; a `check` that is present names an entry in `checks`. A check may now be named `garita`.
