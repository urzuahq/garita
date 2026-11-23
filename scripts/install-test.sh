#!/bin/sh
# shellcheck disable=SC2015,SC2319,SC2010,SC2012,SC2143 # $? after a condition list is the intent; ls is on controlled names
# Tests for scripts/install.sh with no network: curl, wget, uname, ldd, gh and the
# checksum tool are stubs on a private PATH, and the release is a directory of
# archives built here. Every scenario runs under each of bash and dash that exists.
set -u

here="$(cd "$(dirname "$0")" && pwd)"
installer="$here/install.sh"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/garita-install-test.XXXXXX")"
# macOS sets TMPDIR with a trailing slash; the script reports its directory as `pwd` prints it.
tmp="$(CDPATH='' cd -- "$tmp" && pwd)"
trap 'rm -rf "$tmp"' EXIT
fails=0
pass() { echo "ok   $1"; }
flunk() { echo "FAIL $1" >&2; fails=$((fails + 1)); }
expect() { if [ "$2" -eq 0 ]; then pass "$1"; else flunk "$1"; fi; }
has() { grep -q -- "$2" "$1"; }

if command -v sha256sum >/dev/null 2>&1; then real_sum="$(command -v sha256sum)"; else real_sum="$(command -v shasum) -a 256"; fi
sha_of() { $real_sum "$1" | cut -d' ' -f1; }

base="$tmp/bin-base" with_curl="$tmp/bin-curl" with_gh="$tmp/bin-gh" with_wget="$tmp/bin-wget"
hash_sum="$tmp/bin-sum" hash_shasum="$tmp/bin-shasum"
mkdir -p "$base" "$with_curl" "$with_gh" "$with_wget" "$hash_sum" "$hash_shasum"
for tool in mktemp tar gzip sed grep tr cut head rm mv cp chmod mkdir cat ls; do
  src="$(command -v "$tool")" || { echo "install-test: needs $tool on PATH" >&2; exit 1; }
  ln -s "$src" "$base/$tool"
done

cat >"$base/uname" <<'STUB'
#!/bin/sh
case "$1" in -s) echo "$FAKE_OS" ;; -m) echo "$FAKE_ARCH" ;; esac
STUB

cat >"$base/ldd" <<'STUB'
#!/bin/sh
if [ "${FAKE_LIBC:-glibc}" = musl ]; then echo "musl libc (x86_64)" >&2; exit 1; fi
echo "ldd (GNU libc) 2.39"
STUB

cat >"$hash_sum/sha256sum" <<STUB
#!/bin/sh
echo "sha256sum \$*" >>"\$FAKE_LOG"
exec $real_sum "\$@"
STUB
cat >"$hash_shasum/shasum" <<STUB
#!/bin/sh
echo "shasum \$*" >>"\$FAKE_LOG"
[ "\$1 \$2" = "-a 256" ] || exit 2
shift 2
exec $real_sum "\$@"
STUB

# Serves \$FAKE_REL/<tag>/<file>; refuses anything that is not https or that
# was not asked to keep every redirect on https.
cat >"$with_curl/curl" <<'STUB'
#!/bin/sh
echo "curl $*" >>"$FAKE_LOG"
args="$*"
case "$args" in *"--proto =https"*"--proto-redir =https"*) ;; *) echo "curl stub: https was not enforced" >&2; exit 99 ;; esac
out="" url=""
while [ $# -gt 0 ]; do
  case "$1" in
    --output) out="$2"; shift ;;
    --write-out | --proto | --proto-redir | --retry | --connect-timeout | --max-time) shift ;;
    -*) ;;
    *) url="$1" ;;
  esac
  shift
done
case "$url" in https://*) ;; *) echo "curl stub: not https: $url" >&2; exit 99 ;; esac
case "$url" in
  */releases/latest) printf '%s' "${url%/releases/latest}/releases/tag/$FAKE_LATEST" ;;
  */releases/tag/*) [ -d "$FAKE_REL/${url##*/releases/tag/}" ] || exit 22 ;;
  */releases/download/*)
    rest="${url##*/releases/download/}"
    [ -f "$FAKE_REL/$rest" ] || exit 22
    cp "$FAKE_REL/$rest" "$out" ;;
  *) exit 22 ;;
esac
STUB

cat >"$with_wget/wget" <<'STUB'
#!/bin/sh
echo "wget $*" >>"$FAKE_LOG"
out="" url=""
while [ $# -gt 0 ]; do
  case "$1" in -O) out="$2"; shift ;; --tries=* | --timeout=* | --max-redirect=* | --secure-protocol=*) ;; -*) ;; *) url="$1" ;; esac
  shift
done
case "$url" in
  */releases/tag/*) [ -d "$FAKE_REL/${url##*/releases/tag/}" ] || exit 8 ;;
  */releases/download/*)
    rest="${url##*/releases/download/}"
    [ -f "$FAKE_REL/$rest" ] || exit 8
    cp "$FAKE_REL/$rest" "$out" ;;
  *) exit 8 ;;
esac
STUB

# `attestation verify` logs whether the downloaded binary had been run yet.
cat >"$with_gh/gh" <<'STUB'
#!/bin/sh
echo "gh $*" >>"$FAKE_LOG"
case "$1" in
  auth) [ "${FAKE_GH_AUTH:-ok}" = ok ]; exit $? ;;
  attestation) [ -e "$FAKE_RAN" ] && echo "binary-ran-before-verify" >>"$FAKE_LOG"; [ "${FAKE_ATTEST:-ok}" = ok ]; exit $? ;;
esac
STUB
chmod +x "$base"/uname "$base"/ldd "$hash_sum"/sha256sum "$hash_shasum"/shasum "$with_curl"/curl "$with_wget"/wget "$with_gh"/gh

export FAKE_REL="$tmp/rel" FAKE_LOG="$tmp/calls.log" FAKE_RAN="$tmp/binary-ran"
export FAKE_LATEST=v1.2.3 FAKE_OS=Linux FAKE_ARCH=x86_64
LINUX=x86_64-unknown-linux-gnu
MAC=aarch64-apple-darwin
mk_release() { # tag, target, version the binary reports
  d="$tmp/build/$2"
  rm -rf "$d"
  mkdir -p "$d/garita-$2" "$FAKE_REL/$1"
  printf '#!/bin/sh\necho ran >>"%s"\necho "garita %s"\n' "$FAKE_RAN" "$3" >"$d/garita-$2/garita"
  chmod +x "$d/garita-$2/garita"
  printf 'license\n' >"$d/garita-$2/LICENSE"
  printf 'readme\n' >"$d/garita-$2/README.md"
  tar -czf "$FAKE_REL/$1/garita-$2.tar.gz" -C "$d" "garita-$2"
  [ "$1" = v1.2.3 ] || return 0
  if [ "$2" = "$LINUX" ]; then sum="$(sha_of "$FAKE_REL/$1/garita-$2.tar.gz")"; else mac_sum="$(sha_of "$FAKE_REL/$1/garita-$2.tar.gz")"; fi
}
mk_release v1.2.3 $MAC 1.2.3
mk_release v1.2.3 $LINUX 1.2.3
good="$FAKE_REL/v1.2.3/garita-$LINUX.tar.gz"
wrong="0000000000000000000000000000000000000000000000000000000000000000"
mkdir -p "$FAKE_REL/v1.2.4" "$tmp/tmpd" "$tmp/home"

HASH_DIR="$hash_sum"
run() {
  : >"$FAKE_LOG"; rm -f "$FAKE_RAN"
  case "$PATH_MODE" in
    gh) p="$base:$HASH_DIR:$with_curl:$with_gh" ;;
    nogh) p="$base:$HASH_DIR:$with_curl" ;;
    wget) p="$base:$HASH_DIR:$with_wget:$with_gh" ;;
  esac
  PATH="$p${EXTRA_PATH:+:$EXTRA_PATH}" HOME="${RUN_HOME-$tmp/home}" TMPDIR="$tmp/tmpd" "$sh_bin" "$installer" "$@" >"$tmp/out" 2>"$tmp/err"
  rc=$?
}
clean_tmp() { [ -z "$(ls -A "$tmp/tmpd")" ]; }
dest="$tmp/dest"
fresh() { rm -rf "$dest"; }
installed() { [ -x "$dest/garita" ]; }
nothing_installed() { [ ! -e "$dest/garita" ] && { [ ! -d "$dest" ] || [ -z "$(ls -A "$dest")" ]; }; }
old_untouched() { [ "$(cat "$dest/garita")" = old ] && [ "$(ls -A "$dest")" = garita ]; }
seed_old() { fresh; mkdir -p "$dest"; printf 'old' >"$dest/garita"; }

lists_all() {
  for w in --version --dir --sha256 --no-attestation --no-verify GARITA_VERSION GARITA_INSTALL_DIR GARITA_SHA256 GARITA_RELEASE_BASE_URL; do
    has "$tmp/out" "$w" || return 1
  done
}

suite() {
  PATH_MODE=gh; HASH_DIR="$hash_sum"

  # ---- success paths ----
  fresh
  run --version v1.2.3 --dir "$dest/nested/bin"
  [ "$rc" -eq 0 ] && [ -x "$dest/nested/bin/garita" ]
  expect "$label install: attested release installs and the missing directory is created" $?
  [ "$(wc -l <"$tmp/out" | tr -d ' ')" -eq 1 ] &&
    [ "$(cat "$tmp/out")" = "installed garita=v1.2.3 target=$LINUX sha256=$sum verified=attestation path=$dest/nested/bin/garita" ]
  expect "$label install: stdout is exactly the one final line" $?
  [ "$(ls -ld "$dest/nested/bin/garita" | cut -c1-10)" = "-rwxr-xr-x" ]
  expect "$label install: the binary is mode 0755" $?
  [ "$("$dest/nested/bin/garita" --version)" = "garita 1.2.3" ]
  expect "$label install: the installed file is the archive's binary" $?
  has "$FAKE_LOG" "--signer-workflow urzuahq/garita/.github/workflows/publish-release.yml" &&
    has "$FAKE_LOG" "attestation verify .*garita-$LINUX.tar.gz --repo urzuahq/garita"
  expect "$label install: the attestation is pinned to the repository and its publish workflow" $?
  ! has "$FAKE_LOG" binary-ran-before-verify && [ -e "$FAKE_RAN" ]
  expect "$label install: the binary runs only after verification" $?
  has "$FAKE_LOG" "--proto =https --proto-redir =https"
  expect "$label install: curl is held to https, redirects included" $?
  clean_tmp
  expect "$label install: no temporary directory is left behind on success" $?
  [ -z "$(ls -A "$dest/nested/bin" | grep -v '^garita$')" ]
  expect "$label install: no staging file is left in the install directory" $?

  fresh; run --version v1.2.3 --dir "$dest" --sha256 "$(echo "$sum" | tr a-f A-F)"
  [ "$rc" -eq 0 ] && has "$tmp/out" "verified=attestation+sha256 "
  expect "$label install: attestation and a matching checksum are both reported (checksum case-insensitive)" $?

  fresh; run --version v1.2.3 --dir "$dest" --no-attestation --sha256 "$sum"
  [ "$rc" -eq 0 ] && has "$tmp/err" "WARNING: --no-attestation" && has "$tmp/out" "verified=sha256 " && ! has "$FAKE_LOG" "attestation verify"
  expect "$label install: --no-attestation with a checksum installs, warns, and skips the attestation" $?

  fresh; run --version v1.2.3 --dir "$dest" --no-attestation
  [ "$rc" -eq 1 ] && has "$tmp/err" "--no-attestation was passed" && nothing_installed && clean_tmp
  expect "$label install: --no-attestation without a checksum is refused" $?

  fresh; run --dir "$dest"
  [ "$rc" -eq 0 ] && has "$tmp/out" "garita=v1.2.3 " && has "$FAKE_LOG" "releases/latest" && ! has "$FAKE_LOG" "gh api"
  expect "$label install: no version resolves the latest release through the redirect, with no gh API call" $?

  fresh; GARITA_VERSION=v1.2.3 GARITA_INSTALL_DIR="$dest" run
  [ "$rc" -eq 0 ] && installed
  expect "$label install: GARITA_VERSION and GARITA_INSTALL_DIR are honoured" $?

  fresh; GARITA_VERSION=v1.2.3 GARITA_INSTALL_DIR="$tmp/other" run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 0 ] && installed && [ ! -e "$tmp/other" ]
  expect "$label install: a flag overrides its environment variable" $?

  fresh; run --version v1.2.3
  [ "$rc" -eq 0 ] && [ -x "$tmp/home/.local/bin/garita" ]
  expect "$label install: the default directory is \$HOME/.local/bin" $?
  rm -rf "$tmp/home/.local"

  fresh; RUN_HOME='' run --version v1.2.3
  [ "$rc" -eq 2 ] && has "$tmp/err" "HOME is unset" && clean_tmp
  expect "$label install: no HOME and no --dir is a usage error naming --dir" $?

  # ---- platforms ----
  fresh; FAKE_OS=Darwin FAKE_ARCH=arm64 run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 0 ] && has "$tmp/out" "target=$MAC sha256=$mac_sum verified=attestation"
  expect "$label platform: Apple Silicon maps to $MAC" $?
  fresh; FAKE_OS=Linux FAKE_ARCH=amd64 run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 0 ] && has "$tmp/out" "target=$LINUX "
  expect "$label platform: Linux amd64 maps to $LINUX" $?

  for combo in "Linux aarch64" "Darwin x86_64" "FreeBSD amd64"; do
    fresh; FAKE_OS="${combo% *}" FAKE_ARCH="${combo#* }" run --version v1.2.3 --dir "$dest"
    [ "$rc" -eq 1 ] && has "$tmp/err" "no published garita build for ${combo% *}/${combo#* }" &&
      has "$tmp/err" "x86_64-unknown-linux-gnu, aarch64-apple-darwin" && has "$tmp/err" "build from source" &&
      [ ! -d "$dest" ] && clean_tmp && ! has "$FAKE_LOG" "^curl "
    expect "$label platform: $combo is refused naming the published targets and building from source" $?
  done

  fresh; FAKE_LIBC=musl run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 1 ] && has "$tmp/err" "musl" && has "$tmp/err" "build from source" && [ ! -d "$dest" ] && ! has "$FAKE_LOG" "^curl " && clean_tmp
  expect "$label platform: a musl Linux is refused before any download" $?
  fresh; FAKE_LIBC=musl GARITA_SKIP_LIBC_CHECK=1 run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 0 ] && installed
  expect "$label platform: the libc check override lets a musl-reporting system through" $?
  fresh; FAKE_OS=Darwin FAKE_ARCH=arm64 FAKE_LIBC=musl run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 0 ]
  expect "$label platform: the libc check does not apply on macOS" $?

  # ---- verification ----
  seed_old
  run --version v1.2.3 --dir "$dest" --sha256 "$wrong"
  [ "$rc" -eq 1 ] && has "$tmp/err" "checksum mismatch" && old_untouched && [ ! -e "$FAKE_RAN" ] && clean_tmp
  expect "$label verify: a checksum mismatch is refused, the existing binary is untouched, nothing ran" $?
  ! has "$FAKE_LOG" "attestation verify"
  expect "$label verify: a checksum mismatch stops before the attestation call" $?

  seed_old
  tampered="$tmp/build/tampered"; rm -rf "$tampered"; mkdir -p "$tampered/garita-$LINUX"
  printf '#!/bin/sh\necho ran >>"%s"\necho "garita 1.2.3"\n# tampered\n' "$FAKE_RAN" >"$tampered/garita-$LINUX/garita"
  chmod +x "$tampered/garita-$LINUX/garita"
  cp "$good" "$tmp/genuine.tar.gz"
  tar -czf "$good" -C "$tampered" "garita-$LINUX"
  run --version v1.2.3 --dir "$dest" --sha256 "$sum"
  [ "$rc" -eq 1 ] && has "$tmp/err" "checksum mismatch" && old_untouched && [ ! -e "$FAKE_RAN" ] && clean_tmp
  expect "$label verify: a tampered archive is refused against the genuine checksum" $?
  FAKE_ATTEST=fail run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 1 ] && has "$tmp/err" "attestation failed" && old_untouched && [ ! -e "$FAKE_RAN" ] && clean_tmp
  expect "$label verify: a tampered archive that fails its attestation is refused" $?
  cp "$tmp/genuine.tar.gz" "$good"

  seed_old; FAKE_ATTEST=fail run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 1 ] && has "$tmp/err" "build provenance attestation failed" && old_untouched && [ ! -e "$FAKE_RAN" ] && clean_tmp
  expect "$label verify: a failed attestation is refused, existing binary untouched, nothing ran" $?
  seed_old; FAKE_ATTEST=fail run --version v1.2.3 --dir "$dest" --sha256 "$sum"
  [ "$rc" -eq 1 ] && has "$tmp/err" "attestation failed" && old_untouched && clean_tmp
  expect "$label verify: a matching checksum does not excuse a failed attestation" $?

  seed_old; FAKE_GH_AUTH=fail run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 1 ] && has "$tmp/err" "gh is not logged in" && has "$tmp/err" "--sha256" && old_untouched && clean_tmp
  expect "$label verify: gh installed but not logged in is refused without a checksum" $?
  fresh; FAKE_GH_AUTH=fail run --version v1.2.3 --dir "$dest" --sha256 "$sum"
  [ "$rc" -eq 0 ] && has "$tmp/out" "verified=sha256 " && ! has "$FAKE_LOG" "attestation verify"
  expect "$label verify: gh not logged in falls back to a matching checksum" $?

  # ---- no gh ----
  PATH_MODE=nogh
  seed_old; run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 1 ] && has "$tmp/err" "gh is not installed" && has "$tmp/err" "--sha256" && has "$tmp/err" "--no-verify" &&
    old_untouched && [ ! -e "$FAKE_RAN" ] && clean_tmp
  expect "$label no-gh: without gh and without a checksum it fails and says what to do" $?
  run --version v1.2.3 --dir "$dest" --sha256 "$wrong"
  [ "$rc" -eq 1 ] && has "$tmp/err" "checksum mismatch" && old_untouched && clean_tmp
  expect "$label no-gh: a wrong checksum is refused" $?
  fresh; run --version v1.2.3 --dir "$dest" --sha256 "$sum"
  [ "$rc" -eq 0 ] && has "$tmp/out" "verified=sha256 " && installed && clean_tmp
  expect "$label no-gh: an explicit checksum installs" $?
  fresh; GARITA_SHA256="$sum" run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 0 ] && installed
  expect "$label no-gh: GARITA_SHA256 is the same as --sha256" $?
  fresh; run --version v1.2.3 --dir "$dest" --no-verify
  [ "$rc" -eq 0 ] && has "$tmp/err" "WARNING: --no-verify" && has "$tmp/out" "verified=none " && installed && clean_tmp
  expect "$label no-gh: --no-verify installs, warns, and reports verified=none" $?
  PATH_MODE=gh; fresh; run --version v1.2.3 --dir "$dest" --no-verify
  [ "$rc" -eq 0 ] && has "$tmp/out" "verified=none " && ! has "$FAKE_LOG" "attestation verify"
  expect "$label no-verify: with gh ready the attestation is skipped" $?
  fresh; run --version v1.2.3 --dir "$dest" --no-verify --sha256 "$wrong"
  [ "$rc" -eq 1 ] && has "$tmp/err" "checksum mismatch" && nothing_installed
  expect "$label no-verify: a wrong checksum still refuses" $?

  HASH_DIR="$hash_shasum"; PATH_MODE=nogh
  fresh; run --version v1.2.3 --dir "$dest" --sha256 "$sum"
  [ "$rc" -eq 0 ] && has "$FAKE_LOG" "^shasum -a 256 " && ! has "$FAKE_LOG" "^sha256sum "
  expect "$label checksum: shasum -a 256 is used when sha256sum is absent" $?
  run --version v1.2.3 --dir "$dest" --sha256 "$wrong"
  [ "$rc" -eq 1 ] && has "$tmp/err" "checksum mismatch"
  expect "$label checksum: a mismatch is caught through shasum too" $?
  HASH_DIR="$hash_sum"; PATH_MODE=gh

  # ---- the reported version ----
  for reported in 11.2.35 1.2.3-rc.1 1.2.30 9.9.9; do
    seed_old; mk_release v1.2.3 $LINUX "$reported"
    run --version v1.2.3 --dir "$dest"
    [ "$rc" -eq 1 ] && has "$tmp/err" "expected exactly 'garita 1.2.3'" && old_untouched && clean_tmp
    expect "$label version: a binary reporting $reported instead of 1.2.3 is refused, existing binary untouched" $?
  done
  fresh; mk_release v1.2.3 $LINUX 1.2.3
  run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 0 ] && installed
  expect "$label version: a binary reporting exactly the requested version installs" $?

  # ---- the release state ----
  seed_old; run --version v1.2.4 --dir "$dest"
  [ "$rc" -eq 1 ] && has "$tmp/err" "has no binary for $LINUX yet" && ! has "$tmp/err" "could not download" && old_untouched && clean_tmp
  expect "$label release: a release without binaries says it has none yet" $?
  seed_old; run --version v9.9.9 --dir "$dest"
  [ "$rc" -eq 1 ] && has "$tmp/err" "could not download" && has "$tmp/err" "v9.9.9" && ! has "$tmp/err" "no binary" && old_untouched && clean_tmp
  expect "$label release: a tag with no release fails naming the cause" $?
  seed_old; FAKE_LATEST=v1.2.4 run --dir "$dest"
  [ "$rc" -eq 1 ] && has "$tmp/err" "release v1.2.4 has no binary" && old_untouched && clean_tmp
  expect "$label release: a latest release without binaries says it has none yet" $?
  PATH_MODE=wget; seed_old; run --version v1.2.4 --dir "$dest" --no-verify
  [ "$rc" -eq 1 ] && has "$tmp/err" "has no binary for $LINUX yet" && old_untouched && clean_tmp
  expect "$label release: the no-binary message also comes through wget" $?
  PATH_MODE=gh

  # ---- usage ----
  fresh
  for bad in latest 1.2.3 v1.2 "v1.2.3;rm" ""; do
    run --version "$bad" --dir "$dest"
    [ "$rc" -ne 0 ] && nothing_installed && ! has "$FAKE_LOG" "^curl "
    expect "$label usage: --version '$bad' is not an exact tag and is refused" $?
  done
  run --version
  [ "$rc" -eq 2 ]
  expect "$label usage: --version without a value exits 2" $?
  run --bogus
  [ "$rc" -eq 2 ] && has "$tmp/err" "unknown argument" && [ ! -s "$tmp/out" ]
  expect "$label usage: an unknown flag exits 2" $?
  run --sha256 abc --dir "$dest"
  [ "$rc" -eq 2 ] && has "$tmp/err" "64 hex"
  expect "$label usage: a malformed checksum exits 2" $?
  GARITA_RELEASE_BASE_URL=http://example.test run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 1 ] && has "$tmp/err" "must start with https://" && [ ! -s "$FAKE_LOG" ] && nothing_installed
  expect "$label usage: a non-https release base URL is refused before any request" $?
  GARITA_RELEASE_BASE_URL=https://mirror.test run --version v1.2.3 --dir "$dest" --no-verify
  [ "$rc" -eq 0 ] && has "$FAKE_LOG" "https://mirror.test/urzuahq/garita/releases/download/v1.2.3/"
  expect "$label usage: an https base URL is used for the download" $?
  run --help
  [ "$rc" -eq 0 ] && lists_all
  expect "$label usage: --help lists every flag and environment variable" $?

  # ---- PATH note ----
  fresh; run --version v1.2.3 --dir "$dest"
  has "$tmp/err" "$dest is not on PATH"
  expect "$label path: a directory off PATH is noted on stderr" $?
  fresh; EXTRA_PATH="$dest" run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 0 ] && ! has "$tmp/err" "not on PATH"
  expect "$label path: no note when the directory is on PATH" $?

  # ---- wget ----
  PATH_MODE=wget
  fresh; run --version v1.2.3 --dir "$dest"
  [ "$rc" -eq 0 ] && has "$FAKE_LOG" "^wget " && ! has "$FAKE_LOG" "^curl " && has "$tmp/out" "verified=attestation "
  expect "$label download: wget is used when curl is absent" $?
  PATH_MODE=gh

  clean_tmp
  expect "$label cleanup: no scenario left a temporary file" $?
}

ran=0
for sh in bash dash; do
  sh_bin="$(command -v "$sh")" || { echo "notice: $sh is not installed; its run is skipped"; continue; }
  ran=$((ran + 1))
  label="[$sh]"
  suite
done
[ "$ran" -gt 0 ] || flunk "no bash or dash to run the installer under"

[ "$fails" -eq 0 ] && echo "install.sh: all tests passed" || { echo "install.sh: $fails test(s) failed" >&2; exit 1; }
