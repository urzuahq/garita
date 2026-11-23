#!/bin/sh
# shellcheck disable=SC2015 # every || branch is a failing exit
# Installs a garita release binary after verifying it. POSIX sh.
# The final stdout line is the only stdout output; exit codes: 0 installed, 1 refused or failed, 2 bad usage.
set -eu

# One function called on the last line, so a download cut short while piped to sh runs nothing.
main() {
repo="urzuahq/garita"
base_url="${GARITA_RELEASE_BASE_URL:-https://github.com}"
install_doc="https://github.com/$repo/blob/main/docs/reference/install.md"

version="${GARITA_VERSION:-}"
dir="${GARITA_INSTALL_DIR:-}"
expected_sha="${GARITA_SHA256:-}"
no_attestation=0
no_verify=0

say() { printf 'garita-install: %s\n' "$1" >&2; }
fail() { printf 'garita-install: error: %s\n' "$1" >&2; exit 1; }
usage_error() { printf 'garita-install: error: %s (see --help)\n' "$1" >&2; exit 2; }

usage() {
  cat <<'USAGE'
Usage: install.sh [--version vX.Y.Z] [--dir PATH] [--sha256 HEX] [--no-attestation] [--no-verify] [--help]

Installs the garita binary for this platform (x86_64 Linux or Apple Silicon macOS).

Flags (a flag overrides its environment variable):
  --version vX.Y.Z   exact release tag (env GARITA_VERSION); default: the latest release
  --dir PATH         install directory, created if missing (env GARITA_INSTALL_DIR); default: $HOME/.local/bin
  --sha256 HEX       expected sha256 of the archive garita-<target>.tar.gz (env GARITA_SHA256)
  --no-attestation   skip the build provenance check; --sha256 is then required
  --no-verify        install with no verification at all; use only if you accept that
  --help             print this text

Verification, before anything is installed:
  gh installed and logged in   the build provenance attestation must verify
  otherwise                    --sha256 must match, or pass --no-verify
  --sha256 given               it must match, in every mode

Environment: GARITA_VERSION, GARITA_INSTALL_DIR, GARITA_SHA256, GARITA_RELEASE_BASE_URL
(default https://github.com; must be https, for a mirror), and GARITA_SKIP_LIBC_CHECK (any value
skips the musl check).

Last stdout line: installed garita=<tag> target=<target> sha256=<hex> verified=<how> path=<file>
Exit codes: 0 installed, 1 refused or failed, 2 bad usage.
USAGE
}

while [ $# -gt 0 ]; do
  case "$1" in
    --help | -h) usage; exit 0 ;;
    --version)
      [ $# -ge 2 ] && [ -n "$2" ] || usage_error "--version needs a value like v0.1.0 (an empty one would silently mean the latest release)"
      version="$2"; shift 2 ;;
    --dir)
      [ $# -ge 2 ] || usage_error "--dir needs a path"
      dir="$2"; shift 2 ;;
    --sha256)
      [ $# -ge 2 ] && [ -n "$2" ] || usage_error "--sha256 needs a 64-character hex digest (an empty one would silently mean no checksum)"
      expected_sha="$2"; shift 2 ;;
    --no-attestation) no_attestation=1; shift ;;
    --no-verify) no_verify=1; shift ;;
    *) usage_error "unknown argument '$1'" ;;
  esac
done

if [ -z "$dir" ]; then
  [ -n "${HOME:-}" ] || usage_error "no install directory: HOME is unset; pass --dir PATH"
  dir="$HOME/.local/bin"
fi

case "$base_url" in
  https://*) ;;
  *) fail "GARITA_RELEASE_BASE_URL must start with https:// (got '$base_url')" ;;
esac

if [ -n "$expected_sha" ]; then
  expected_sha="$(printf '%s' "$expected_sha" | tr 'A-F' 'a-f')"
  printf '%s' "$expected_sha" | grep -Eq '^[0-9a-f]{64}$' ||
    usage_error "--sha256 must be 64 hex characters"
fi

is_tag() { printf '%s' "$1" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$'; }

if [ -n "$version" ] && ! is_tag "$version"; then
  usage_error "version '$version' is not an exact release tag like v0.1.0; omit it for the latest release"
fi

os="$(uname -s)"
arch="$(uname -m)"
case "$os/$arch" in
  Linux/x86_64 | Linux/amd64) target="x86_64-unknown-linux-gnu" ;;
  Darwin/arm64 | Darwin/aarch64) target="aarch64-apple-darwin" ;;
  *) fail "no published garita build for $os/$arch (published: x86_64-unknown-linux-gnu, aarch64-apple-darwin); build from source: $install_doc#build-from-source" ;;
esac

if [ "$os" = Linux ] && [ -z "${GARITA_SKIP_LIBC_CHECK:-}" ]; then
  if ls /lib/ld-musl-* >/dev/null 2>&1 || ldd --version 2>&1 | grep -qi musl; then
    fail "this is a musl system (Alpine or similar) and the published Linux build needs glibc; build from source: $install_doc#build-from-source"
  fi
fi

if command -v curl >/dev/null 2>&1; then
  downloader=curl
elif command -v wget >/dev/null 2>&1; then
  downloader=wget
else
  fail "neither curl nor wget is installed; install one, or download the archive by hand: $install_doc"
fi

# wget cannot be held to https across redirects; only verification catches a downgrade there.
fetch() { # url, destination file
  if [ "$downloader" = curl ]; then
    curl --fail --silent --show-error --location --retry 2 \
      --connect-timeout 10 --max-time 300 \
      --proto '=https' --proto-redir '=https' --tlsv1.2 --output "$2" "$1"
  else
    wget --quiet --tries=3 --timeout=30 --max-redirect=5 --secure-protocol=TLSv1_2 -O "$2" "$1"
  fi
}

latest_tag() {
  url="$base_url/$repo/releases/latest"
  if [ "$downloader" = curl ]; then
    location="$(curl --silent --show-error --connect-timeout 10 --max-time 30 \
      --proto '=https' --proto-redir '=https' --tlsv1.2 \
      --output /dev/null --write-out '%{redirect_url}' "$url" || true)"
  else
    location="$(wget --quiet --tries=3 --timeout=30 --server-response --max-redirect=0 --secure-protocol=TLSv1_2 \
      -O /dev/null "$url" 2>&1 | sed -n 's/^ *[Ll]ocation: *//p' | tr -d '\r' | head -n 1 || true)"
  fi
  printf '%s' "${location##*/}"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    fail "neither sha256sum nor shasum is installed, so the archive cannot be checked"
  fi
}

work=""
staged=""
cleanup() {
  [ -z "$staged" ] || rm -f "$staged"
  [ -z "$work" ] || rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

work="$(mktemp -d "${TMPDIR:-/tmp}/garita-install.XXXXXX")" || fail "could not create a temporary directory under ${TMPDIR:-/tmp}"

if [ -z "$version" ]; then
  version="$(latest_tag)"
  is_tag "$version" ||
    fail "could not resolve the latest release from $base_url/$repo/releases/latest; pass --version vX.Y.Z"
fi

archive_name="garita-$target.tar.gz"
archive="$work/$archive_name"
url="$base_url/$repo/releases/download/$version/$archive_name"
say "downloading $version for $target"
if ! fetch "$url" "$archive"; then
  # A release page exists a few minutes before its binaries are attached.
  if fetch "$base_url/$repo/releases/tag/$version" /dev/null 2>/dev/null; then
    fail "the release $version has no binary for $target yet (its archives are attached after it is created); try again in a few minutes, or build from source: $install_doc#build-from-source"
  fi
  fail "could not download $url; check that the tag $version exists and that this machine can reach $base_url"
fi

actual_sha="$(sha256_of "$archive")"
verified=""

if [ -n "$expected_sha" ] && [ "$actual_sha" != "$expected_sha" ]; then
  fail "checksum mismatch for $archive_name: expected $expected_sha, got $actual_sha; nothing installed"
fi
[ -z "$expected_sha" ] || verified="sha256"

gh_ready=0
gh_note=""
if [ "$no_attestation" -eq 1 ]; then
  gh_note="--no-attestation was passed"
elif ! command -v gh >/dev/null 2>&1; then
  gh_note="gh is not installed"
elif ! gh auth status >/dev/null 2>&1; then
  gh_note="gh is not logged in (run gh auth login, or set GH_TOKEN)"
else
  gh_ready=1
fi

if [ "$no_verify" -eq 1 ]; then
  say "WARNING: --no-verify: the build provenance of $archive_name is NOT checked${expected_sha:+ (the sha256 you gave matched)}"
  [ -n "$verified" ] || verified="none"
elif [ "$gh_ready" -eq 1 ]; then
  gh attestation verify "$archive" --repo "$repo" \
    --signer-workflow "$repo/.github/workflows/publish-release.yml" >&2 ||
    fail "build provenance attestation failed for $archive_name ($actual_sha); nothing installed"
  if [ -n "$verified" ]; then verified="attestation+sha256"; else verified="attestation"; fi
elif [ -n "$expected_sha" ]; then
  if [ "$no_attestation" -eq 1 ]; then
    say "WARNING: --no-attestation: build provenance is NOT checked; only the sha256 you gave"
  fi
else
  fail "cannot verify $archive_name: $gh_note. Install gh and log in (gh auth login), or pass the expected checksum with --sha256 <hex> (or GARITA_SHA256), or pass --no-verify to install without verification; nothing installed"
fi

mkdir -p -- "$dir" || fail "could not create the install directory $dir"
dir="$(CDPATH='' cd -- "$dir" && pwd)"
[ ! -d "$dir/garita" ] || fail "$dir/garita is a directory; pass --dir with another directory"

mkdir "$work/x"
tar -xzf "$archive" -C "$work/x" "garita-$target/garita" 2>/dev/null ||
  fail "the archive does not contain garita-$target/garita; nothing installed"
bin="$work/x/garita-$target/garita"
{ [ -f "$bin" ] && [ ! -L "$bin" ]; } || fail "garita-$target/garita in the archive is not a regular file; nothing installed"

# Staged beside the destination so the final rename is atomic.
staged="$(mktemp "$dir/.garita.XXXXXX")" || fail "could not write to $dir; pass --dir with a writable directory"
cp "$bin" "$staged" && chmod 0755 "$staged" || fail "could not write to $dir; pass --dir with a writable directory"

want="garita ${version#v}"
reported="$("$staged" --version 2>&1 || true)"
if [ "$reported" != "$want" ]; then
  fail "the binary reports '$reported', expected exactly '$want'; nothing installed (a binary that does not run may mean a musl system)"
fi

mv -f "$staged" "$dir/garita"
staged=""

case ":${PATH:-}:" in
  *":$dir:"*) ;;
  *) say "note: $dir is not on PATH; add it with: export PATH=\"$dir:\$PATH\"" ;;
esac

printf 'installed garita=%s target=%s sha256=%s verified=%s path=%s\n' \
  "$version" "$target" "$actual_sha" "$verified" "$dir/garita"
}

main "$@"
