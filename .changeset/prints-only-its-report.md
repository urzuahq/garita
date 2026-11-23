---
default: minor
---

garita prints nothing but its report.

The summary on stderr is gone; the report on stdout is the only output. A receipt that could not be written, read or pruned is now an `info` finding about the run instead of a line on stderr.
