---
default: patch
---

An install script that verifies what it installs.

One command downloads the release for your platform and checks its build provenance with `gh`, or a sha256 you pin, before it installs anything. See `docs/reference/install.md`.
