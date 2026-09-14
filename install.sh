#!/usr/bin/env sh
#
# mazet installer for Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/LeTuR/mazet/main/install.sh | sh
#
# POSIX sh, not bash: this is piped straight into whatever `/bin/sh` the
# machine has. Standard tools only (curl or wget, tar, sha256sum or shasum),
# nothing interactive, and a trap that removes the download directory on every
# exit path.
#
# The seven target triples, the archive names and the checksum file are
# `.github/workflows/cd.yml`'s output and are the contract between the two.
# See docs/RELEASING.md; do not change one side alone.
#
# On Linux this installs the musl build, never the gnu one, even though both
# are published. See resolve_target.
#
# Configuration, all optional and all prefixed: a bare `VERSION` or
# `INSTALL_DIR` in the caller's environment must not be able to steer an
# installer they piped into a shell.
#
#   MAZET_VERSION       tag to install, e.g. v0.1.0  (default: latest release)
#   MAZET_INSTALL_DIR   where the binary goes        (default: ~/.local/bin)
#   MAZET_REPO          owner/name to install from   (default: LeTuR/mazet)

set -eu

MAZET_REPO="${MAZET_REPO:-LeTuR/mazet}"
MAZET_INSTALL_DIR="${MAZET_INSTALL_DIR:-$HOME/.local/bin}"
MAZET_VERSION="${MAZET_VERSION:-}"

# Set by main() once the download directory exists; the trap reads it.
MAZET_TMP=""

cleanup() {
    if [ -n "$MAZET_TMP" ] && [ -d "$MAZET_TMP" ]; then
        rm -rf "$MAZET_TMP"
    fi
}
trap cleanup EXIT INT TERM

# Everything this script says goes to stderr: stdout belongs to the caller, and
# `curl ... | sh > log` should still show progress on the terminal.
say() { printf '%s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# --- pure helpers -----------------------------------------------------------
# Every function down to `fetch` is a value in, a value out, so the test suite
# can exercise the mapping and the parsing without performing an install.

# `uname -s` and `uname -m` in, one of cd.yml's seven target triples out.
# Anything else is refused by name rather than guessed at: installing the wrong
# binary is worse than not installing one.
#
# On Linux the answer is always the musl build. A gnu binary carries a floor --
# the glibc it was linked against, and never anything older -- set by whatever
# the release runner happened to be: v0.1.0's gnu asset was built against
# glibc 2.39 and would not start on Debian 12, which has 2.36. A musl build is
# statically linked and has no floor, so it is the only one an installer can
# hand a machine it knows nothing about. mazet spawns processes and reads
# directories and does no name resolution, so it wants nothing that glibc's
# NSS gives and musl's does not. The gnu archives stay published for anyone
# who has a reason to prefer one.
resolve_target() {
    case "$1" in
        Linux)
            case "$2" in
                x86_64 | amd64) printf '%s\n' "x86_64-unknown-linux-musl" ;;
                aarch64 | arm64) printf '%s\n' "aarch64-unknown-linux-musl" ;;
                *) die "mazet publishes no Linux build for $2 (detected with \`uname -m\`). Built targets are x86_64 and aarch64; install from source with \`cargo install --git https://github.com/$MAZET_REPO\`." ;;
            esac
            ;;
        Darwin)
            case "$2" in
                x86_64) printf '%s\n' "x86_64-apple-darwin" ;;
                arm64 | aarch64) printf '%s\n' "aarch64-apple-darwin" ;;
                *) die "mazet publishes no macOS build for $2 (detected with \`uname -m\`). Built targets are x86_64 and arm64." ;;
            esac
            ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT)
            die "this script installs the Linux and macOS builds; $1 is Windows. Run the PowerShell installer instead: irm https://raw.githubusercontent.com/$MAZET_REPO/main/install.ps1 | iex"
            ;;
        *)
            die "mazet publishes no build for $1 (detected with \`uname -s\`). Supported systems are Linux and macOS, plus Windows through install.ps1."
            ;;
    esac
}

# cd.yml names every asset after the tag, `v` included, so a caller who asked
# for `0.1.0` means `v0.1.0` and should not get a 404 for the difference.
normalize_version() {
    case "$1" in
        v*) printf '%s\n' "$1" ;;
        *) printf '%s\n' "v$1" ;;
    esac
}

# The tag out of the releases API's JSON. Deliberately not a JSON parser: `jq`
# is not a standard tool and this reads one well-known key.
tag_from_api_json() {
    sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1
}

# The tag out of the HTML that github.com/<repo>/releases/latest redirects to.
# This is the fallback for the API's unauthenticated rate limit, which a whole
# office behind one address reaches on its own.
tag_from_release_html() {
    sed -n 's|.*/releases/tag/\(v[0-9][0-9A-Za-z.+-]*\).*|\1|p' | head -1
}

# The hash for one archive out of `mazet-<tag>-checksums.txt`.
#
# cd.yml produces that file with `sha256sum ./*.tar.gz ./*.zip` run from inside
# the assets directory, so every name in it carries a `./` prefix; a file
# written with `sha256sum -b` would mark binary mode with a leading `*`. Both
# are stripped and the comparison is a plain string equality, which needs no
# regex escaping of a name that is full of dots.
checksum_for() {
    _cf_want="$2"
    _cf_hash=""
    while read -r _cf_h _cf_n; do
        [ -n "$_cf_h" ] || continue
        _cf_n="${_cf_n#\*}"
        _cf_n="${_cf_n#./}"
        if [ "$_cf_n" = "$_cf_want" ]; then
            _cf_hash="$_cf_h"
            break
        fi
    done <"$1"
    [ -n "$_cf_hash" ] || die "$_cf_want is not listed in the checksum file. The release may still be uploading; check https://github.com/$MAZET_REPO/releases"
    printf '%s\n' "$_cf_hash"
}

sha256_of() {
    if have sha256sum; then
        sha256sum "$1" | cut -d' ' -f1
    elif have shasum; then
        shasum -a 256 "$1" | cut -d' ' -f1
    else
        die "sha256sum or shasum is required to verify the download, and neither is on PATH. Refusing to install an unverified binary."
    fi
}

# The reason the checksum file is published at all. An installer that downloads
# and runs without checking it is worse than no installer.
verify_checksum() {
    _vc_actual=$(sha256_of "$1" | tr 'ABCDEF' 'abcdef')
    _vc_expected=$(printf '%s' "$2" | tr 'ABCDEF' 'abcdef')
    if [ "$_vc_actual" != "$_vc_expected" ]; then
        die "checksum mismatch for $1
  expected $_vc_expected
  got      $_vc_actual
Nothing was installed."
    fi
}

# Is this exact directory a component of PATH? A substring test would call
# ~/.local/bin present because ~/.local/bin/extra is, and a trailing separator
# names the same directory rather than a different one.
on_path() {
    _op_dir="${1%/}"
    [ -n "$_op_dir" ] || _op_dir="/"
    case ":${PATH:-}:" in
        *":$_op_dir:"*) return 0 ;;
        *) return 1 ;;
    esac
}

# --- the world --------------------------------------------------------------

# The leaf functions below each refuse when their own tool is missing, but the
# first of them to run is inside a command substitution in a pipeline, where an
# `exit` only leaves the subshell and the caller reports the wrong thing. So
# ask once, up front, and fail with the message that names what to install.
require_tools() {
    have curl || have wget ||
        die "curl or wget is required to download mazet, and neither is on PATH."
    have sha256sum || have shasum ||
        die "sha256sum or shasum is required to verify the download, and neither is on PATH. Refusing to install an unverified binary."
    have tar || die "tar is required to unpack the release archive, and it is not on PATH."
}

fetch_stdout() {
    if have curl; then
        curl -fsSL --proto '=https' --tlsv1.2 "$1"
    elif have wget; then
        wget -q -O - "$1"
    else
        die "curl or wget is required to download mazet, and neither is on PATH."
    fi
}

fetch() {
    if have curl; then
        curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
    elif have wget; then
        wget -q -O "$2" "$1"
    else
        die "curl or wget is required to download mazet, and neither is on PATH."
    fi
}

latest_version() {
    _lv_tag=$(fetch_stdout "https://api.github.com/repos/$MAZET_REPO/releases/latest" 2>/dev/null | tag_from_api_json || true)
    if [ -z "$_lv_tag" ]; then
        _lv_tag=$(fetch_stdout "https://github.com/$MAZET_REPO/releases/latest" 2>/dev/null | tag_from_release_html || true)
    fi
    [ -n "$_lv_tag" ] || die "could not work out the latest release of $MAZET_REPO. Name one instead: MAZET_VERSION=v0.1.0 curl -fsSL https://raw.githubusercontent.com/$MAZET_REPO/main/install.sh | sh"
    printf '%s\n' "$_lv_tag"
}

# Extract the binary and nothing else. The archive also carries LICENSE and
# README.md, which belong nowhere near a bin directory, and the binary lands
# through a rename: replacing a *running* executable by writing over it fails
# with ETXTBSY, and half-writing one is worse than either.
# $1 archive, $2 install directory, $3 an empty directory to unpack into.
install_binary() {
    _ib_stage="$3"
    tar -xzf "$1" -C "$_ib_stage" mazet || die "could not extract mazet from $1."
    mkdir -p "$2" || die "could not create $2."
    chmod +x "$_ib_stage/mazet"
    mv -f "$_ib_stage/mazet" "$2/.mazet.incoming" || die "could not write into $2."
    mv -f "$2/.mazet.incoming" "$2/mazet" || die "could not install into $2."
}

# The install is not finished when the file is in place; it is finished when
# the file runs. A binary whose libc floor is above this machine's installs
# perfectly and then dies on its first run with a message from the dynamic
# linker that names neither mazet nor this installer -- which is exactly how
# v0.1.0 looked to someone on Debian 12. Ask it for its version here, while
# there is still an installer to explain the answer.
# $1 the installed binary, $2 the tag it came from.
verify_runs() {
    if _vr_out=$("$1" --version 2>&1); then
        printf '%s\n' "$_vr_out" | head -1
        return 0
    fi
    say ""
    say "$1 was installed but does not run on this machine:"
    printf '%s\n' "$_vr_out" | sed 's/^/    /' >&2
    say ""
    die "mazet $2 will not start here, so the install is not usable. The file is
left at $1 so you can look at it. Please report the lines above, with
\`uname -srm\`, at https://github.com/$MAZET_REPO/issues"
}

main() {
    require_tools
    _m_os=$(uname -s)
    _m_arch=$(uname -m)
    _m_target=$(resolve_target "$_m_os" "$_m_arch")
    say "mazet installer"
    say "  platform  $_m_os $_m_arch -> $_m_target"

    if [ -n "$MAZET_VERSION" ]; then
        _m_version=$(normalize_version "$MAZET_VERSION")
    else
        _m_version=$(latest_version)
    fi
    say "  version   $_m_version"

    MAZET_TMP=$(mktemp -d) || die "could not create a temporary directory."

    _m_archive="mazet-$_m_version-$_m_target.tar.gz"
    _m_base="https://github.com/$MAZET_REPO/releases/download/$_m_version"

    say "  fetching  $_m_archive"
    fetch "$_m_base/mazet-$_m_version-checksums.txt" "$MAZET_TMP/checksums.txt" ||
        die "no checksum file for $_m_version. Check https://github.com/$MAZET_REPO/releases/tag/$_m_version"
    fetch "$_m_base/$_m_archive" "$MAZET_TMP/$_m_archive" ||
        die "could not download $_m_archive. Check https://github.com/$MAZET_REPO/releases/tag/$_m_version"

    _m_expected=$(checksum_for "$MAZET_TMP/checksums.txt" "$_m_archive")
    verify_checksum "$MAZET_TMP/$_m_archive" "$_m_expected"
    say "  verified  sha256 $_m_expected"

    mkdir -p "$MAZET_TMP/unpacked"
    install_binary "$MAZET_TMP/$_m_archive" "$MAZET_INSTALL_DIR" "$MAZET_TMP/unpacked"

    _m_reported=$(verify_runs "$MAZET_INSTALL_DIR/mazet" "$_m_version")
    say "  runs      $_m_reported"

    say ""
    say "mazet $_m_version is installed at $MAZET_INSTALL_DIR/mazet"
    if on_path "$MAZET_INSTALL_DIR"; then
        say "Run \`mazet\` to see what this machine knows about."
    else
        say ""
        say "$MAZET_INSTALL_DIR is not on your PATH. Add it:"
        say "    export PATH=\"$MAZET_INSTALL_DIR:\$PATH\""
        say "and put that line in your shell's startup file."
    fi
    say ""
    say "Next: \`mazet init\` in a directory tree to bind it to an Azure identity"
    say "of its own, and \`eval \"\$(mazet hook bash)\"\` in your shell startup"
    say "file so bare \`az\` follows it."
}

# Dot-source with MAZET_INSTALL_TEST set to get the helpers without installing.
if [ -z "${MAZET_INSTALL_TEST:-}" ]; then
    main "$@"
fi
