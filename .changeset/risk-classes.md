---
default: patch
---

A change is matched to one of the repository's risk classes, and the run report says which.

A `risk` section in `.garita/config.yaml` lists the classes, lowest first, and the paths each covers. `garita run` reads the policy from the merge base, so a change cannot lower its own class, and reports `risk`: the class, how it was set, the paths that set it, how many people must approve, and whether the checks that ran passed. `.garita/**` and `.github/workflows/**` always match the highest class. When the merge base's policy cannot be read, the class is `null`, a person must approve, and the run goes on. A report says `risk: null` when the merge base declares no policy. A class may not yet require a system above 0.
