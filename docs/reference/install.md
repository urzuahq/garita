# Install

## Install a release

Releases are published at <https://github.com/urzuahq/garita/releases> for two targets:
`x86_64-unknown-linux-gnu` and `aarch64-apple-darwin` (Apple Silicon). Each archive holds
`garita-<target>/garita`, `LICENSE` and `README.md`. The install script
[`scripts/install.sh`](../../scripts/install.sh) picks the target, downloads the archive, verifies it
and installs it to `$HOME/.local/bin` (no `sudo`). It installs nothing, and runs nothing it
downloaded, until the archive has verified. The one exception is `--no-verify`, which skips
verification and so runs the downloaded binary's version check unverified.

### On a machine with `gh`

`gh` verifies the archive's build provenance, so no checksum is needed. This installs the latest
release:

```sh
tag=$(gh release view --repo urzuahq/garita --json tagName --jq .tagName)
curl -fsSL --proto '=https' --tlsv1.2 "https://raw.githubusercontent.com/urzuahq/garita/$tag/scripts/install.sh" | sh -s -- --version "$tag"
```

To read the script before running it, save it with `curl -fsSLO` and run `sh install.sh`. The script
verifies the archive it downloads, but nothing verifies the script itself: it is fetched over HTTPS
from the tag, and a tag can be moved. Where that matters, pin a commit sha in the URL in place of the
tag.

### In CI, or anywhere without `gh`

Pin the version and the archive's sha256, so the install fails if the archive is anything else:

```sh
tag="${GARITA_TAG:?set GARITA_TAG to an exact release tag, such as vX.Y.Z}"
sha256="${GARITA_SHA256:?set GARITA_SHA256 to the sha256 of this platform's archive at that tag}"
curl -fsSL --proto '=https' --tlsv1.2 "https://raw.githubusercontent.com/urzuahq/garita/$tag/scripts/install.sh" \
  | sh -s -- --version "$tag" --sha256 "$sha256"
```

A release has no checksum file. Compute the sha256 of the archive from the release you pin:

```sh
gh release download "$tag" --repo urzuahq/garita --pattern 'garita-*.tar.gz'
sha256sum garita-*.tar.gz # on macOS: shasum -a 256 garita-*.tar.gz
```

## Flags and environment

Run `sh scripts/install.sh --help` for the same list. A flag overrides its environment variable.

| Flag | Environment | Meaning |
|---|---|---|
| `--version vX.Y.Z` | `GARITA_VERSION` | An exact release tag. Default: the latest release. |
| `--dir PATH` | `GARITA_INSTALL_DIR` | Install directory, created if missing. Default: `$HOME/.local/bin`. |
| `--sha256 HEX` | `GARITA_SHA256` | The archive's expected sha256. A mismatch refuses the install. |
| `--no-attestation` | | Skip the provenance check; `--sha256` is then required. |
| `--no-verify` | | Install with no verification, with a warning. Use it only knowingly. |
| `--help` | | Print the usage. |
| | `GARITA_RELEASE_BASE_URL` | Where releases are fetched from, for a mirror. Default: `https://github.com`. Must be https. |
| | `GARITA_SKIP_LIBC_CHECK` | Any value skips the musl check. |

## What the script verifies

- **Verification.** With `gh` installed and logged in, the build provenance attestation must verify
  (`gh attestation verify`, pinned to `urzuahq/garita` and its `publish-release.yml` workflow).
  Without `gh`, or when it is not logged in, the script fails unless you pass `--sha256` or
  `--no-verify`. A checksum you pass must match in every mode. A wrong checksum or a failed
  attestation installs nothing and leaves an existing `garita` untouched.
- **Version.** The binary is written next to its destination and run once with `--version`. It must
  print exactly `garita <version>` for the requested tag; any other string, including one that
  merely contains it, installs nothing. Only then is it renamed into place with mode `0755`.
- **Output.** The last stdout line is
  `installed garita=<tag> target=<target> sha256=<hex> verified=<how> path=<file>`, where `<how>` is
  `attestation`, `attestation+sha256`, `sha256` or `none`. Everything else goes to stderr, including
  a note when the directory is not on `PATH`. Exit codes: `0` installed, `1` refused or failed, `2`
  bad usage.
- **Network.** `curl`, or `wget` when `curl` is absent, over https only. With `curl`, redirects are
  held to https. The latest release is resolved from the `releases/latest` redirect, with no API call
  or token. A release page appears a few minutes before its archives are attached; in that window the
  script says the release has no binary for your platform yet.

## Supported platforms

x86_64 Linux with glibc, and Apple Silicon macOS. On any other platform, including Alpine and other
musl systems, the script fails and points here. There is no Intel Mac, Linux arm64 or Windows build.

## Build from source

You need Rust 1.87 or later.

```sh
git clone https://github.com/urzuahq/garita
cd garita/rust && cargo build --release
```

The binary is `rust/target/release/garita`. Put it on your `PATH`:

```sh
install -m 0755 target/release/garita /usr/local/bin/garita
garita --version
```
