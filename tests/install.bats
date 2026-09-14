#!/usr/bin/env bats
#
# Behaviour of install.sh's pure helpers, plus one end-to-end pass over a real
# archive. Nothing here reaches the network and nothing here greps the source
# for a string: every assertion runs the code.
#
# install.sh guards main() behind MAZET_INSTALL_TEST, so dot-sourcing it
# defines every helper without performing an install. Each case dot-sources in
# its own `sh -c` so the script's `set -eu` never leaks into the test shell.

setup() {
    INSTALLER="${BATS_TEST_DIRNAME}/../install.sh"
    export INSTALLER
}

# Run a snippet with install.sh sourced. `sh`, not bash: the same interpreter
# the documented one-liner pipes into.
sourced() {
    MAZET_INSTALL_TEST=1 sh -c ". \"\$INSTALLER\"; $1"
}

@test "the script parses as POSIX sh" {
    run sh -n "$INSTALLER"
    [ "$status" -eq 0 ]
}

@test "the source is ASCII only, so any shell decodes it the same way" {
    run env LC_ALL=C grep -n '[^ -~	]' "$INSTALLER"
    [ "$status" -ne 0 ]
}

@test "dot-sourcing under MAZET_INSTALL_TEST installs nothing" {
    run sourced "true"
    [ "$status" -eq 0 ]
    [ -z "$output" ]
}

# --- resolve_target ---------------------------------------------------------

# A gnu build carries a glibc floor -- the version it was linked against, and
# never anything older -- so the runner that builds it decides which distros
# can run it. v0.1.0's x86_64 gnu asset was built on glibc 2.39 and would not
# start on Debian 12. The musl builds are statically linked and have no floor,
# which is the only property that makes an unknown machine a safe target.
@test "resolve_target maps Linux x86_64 to the musl target" {
    run sourced "resolve_target Linux x86_64"
    [ "$status" -eq 0 ]
    [ "$output" = "x86_64-unknown-linux-musl" ]
}

@test "resolve_target maps Linux aarch64 to the musl target" {
    run sourced "resolve_target Linux aarch64"
    [ "$status" -eq 0 ]
    [ "$output" = "aarch64-unknown-linux-musl" ]
}

@test "resolve_target maps Linux arm64 and amd64, the names some unames use" {
    run sourced "resolve_target Linux arm64"
    [ "$output" = "aarch64-unknown-linux-musl" ]
    run sourced "resolve_target Linux amd64"
    [ "$output" = "x86_64-unknown-linux-musl" ]
}

@test "resolve_target never hands a Linux machine a glibc-linked build" {
    for arch in x86_64 amd64 aarch64 arm64; do
        run sourced "resolve_target Linux $arch"
        [ "$status" -eq 0 ]
        [[ "$output" != *"-gnu" ]]
    done
}

@test "resolve_target maps macOS arm64, which is what uname -m says there" {
    run sourced "resolve_target Darwin arm64"
    [ "$status" -eq 0 ]
    [ "$output" = "aarch64-apple-darwin" ]
}

@test "resolve_target maps macOS x86_64" {
    run sourced "resolve_target Darwin x86_64"
    [ "$status" -eq 0 ]
    [ "$output" = "x86_64-apple-darwin" ]
}

@test "resolve_target refuses an unbuilt architecture and names it" {
    run sourced "resolve_target Linux riscv64"
    [ "$status" -ne 0 ]
    [[ "$output" == *"riscv64"* ]]
    [[ "$output" == *"no Linux build"* ]]
}

@test "resolve_target refuses an unbuilt system and names it" {
    run sourced "resolve_target FreeBSD x86_64"
    [ "$status" -ne 0 ]
    [[ "$output" == *"FreeBSD"* ]]
}

@test "resolve_target sends Windows to install.ps1 rather than guessing" {
    run sourced "resolve_target MINGW64_NT-10.0 x86_64"
    [ "$status" -ne 0 ]
    [[ "$output" == *"install.ps1"* ]]
}

@test "resolve_target never resolves a target for an unsupported platform" {
    # The refusal must not also print a triple: a caller that reads stdout
    # would otherwise download something.
    run sourced "resolve_target Linux riscv64 2>/dev/null"
    [ -z "$output" ]
}

# --- normalize_version ------------------------------------------------------

@test "normalize_version leaves a v-prefixed tag alone" {
    run sourced "normalize_version v0.1.0"
    [ "$output" = "v0.1.0" ]
}

@test "normalize_version adds the v the release assets are named with" {
    run sourced "normalize_version 0.1.0"
    [ "$output" = "v0.1.0" ]
}

# --- version parsing --------------------------------------------------------

@test "tag_from_api_json reads tag_name out of the releases API answer" {
    run sourced "printf '%s\n' '{' '  \"url\": \"x\",' '  \"tag_name\": \"v1.2.3\",' '  \"name\": \"mazet v1.2.3\"' '}' | tag_from_api_json"
    [ "$output" = "v1.2.3" ]
}

@test "tag_from_api_json yields nothing for an answer without a tag" {
    run sourced "printf '%s\n' '{\"message\":\"API rate limit exceeded\"}' | tag_from_api_json"
    [ -z "$output" ]
}

@test "tag_from_release_html reads the tag out of the releases page" {
    run sourced "printf '%s\n' '<a href=\"/LeTuR/mazet/releases/tag/v0.4.1\">v0.4.1</a>' | tag_from_release_html"
    [ "$output" = "v0.4.1" ]
}

# --- checksum_for -----------------------------------------------------------

# cd.yml writes the file with `sha256sum ./*.tar.gz ./*.zip` from inside the
# assets directory, so every name in it is `./`-prefixed. These are the shapes
# the parser has to survive.
checksums_fixture() {
    cat >"$BATS_TEST_TMPDIR/checksums.txt" <<EOF
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  ./mazet-v1.2.3-x86_64-unknown-linux-gnu.tar.gz
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  ./mazet-v1.2.3-aarch64-unknown-linux-gnu.tar.gz
cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc  ./mazet-v1.2.3-x86_64-pc-windows-msvc.zip
EOF
}

@test "checksum_for finds the hash for one archive among several" {
    checksums_fixture
    run sourced "checksum_for '$BATS_TEST_TMPDIR/checksums.txt' mazet-v1.2.3-aarch64-unknown-linux-gnu.tar.gz"
    [ "$status" -eq 0 ]
    [ "$output" = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" ]
}

@test "checksum_for tolerates a name with no ./ prefix" {
    printf '%s  %s\n' "$(printf 'd%.0s' $(seq 64))" "mazet-v1.2.3-x86_64-apple-darwin.tar.gz" \
        >"$BATS_TEST_TMPDIR/plain.txt"
    run sourced "checksum_for '$BATS_TEST_TMPDIR/plain.txt' mazet-v1.2.3-x86_64-apple-darwin.tar.gz"
    [ "$status" -eq 0 ]
    [ "$output" = "$(printf 'd%.0s' $(seq 64))" ]
}

@test "checksum_for tolerates sha256sum's binary-mode asterisk" {
    printf '%s *%s\n' "$(printf 'e%.0s' $(seq 64))" "mazet-v1.2.3-x86_64-apple-darwin.tar.gz" \
        >"$BATS_TEST_TMPDIR/binmode.txt"
    run sourced "checksum_for '$BATS_TEST_TMPDIR/binmode.txt' mazet-v1.2.3-x86_64-apple-darwin.tar.gz"
    [ "$status" -eq 0 ]
    [ "$output" = "$(printf 'e%.0s' $(seq 64))" ]
}

@test "checksum_for refuses an archive the file does not list" {
    checksums_fixture
    run sourced "checksum_for '$BATS_TEST_TMPDIR/checksums.txt' mazet-v1.2.3-aarch64-apple-darwin.tar.gz"
    [ "$status" -ne 0 ]
    [[ "$output" == *"aarch64-apple-darwin"* ]]
}

# install.sh resolves Linux to musl, and the fixture above is the asset list of
# a release cut before mazet built a musl target -- v0.1.0's shape exactly. The
# checksum file is that asset list, so this is where the mismatch surfaces, and
# it has to surface as "that release does not carry it", not as a download that
# happened to fail.
@test "a release carrying no musl asset is refused by name" {
    checksums_fixture
    run sourced "
        target=\$(resolve_target Linux x86_64)
        checksum_for '$BATS_TEST_TMPDIR/checksums.txt' \"mazet-v1.2.3-\$target.tar.gz\"
    "
    [ "$status" -ne 0 ]
    [[ "$output" == *"mazet-v1.2.3-x86_64-unknown-linux-musl.tar.gz"* ]]
    [[ "$output" == *"cut before mazet built that target"* ]]
    [[ "$output" == *"install a newer"* ]]
}

@test "checksum_for does not match a name by prefix" {
    # `mazet-v1.2.3-x86_64-unknown-linux-gnu.tar.gz` is in the file; a request
    # for a longer name that starts the same way must not resolve to it.
    checksums_fixture
    run sourced "checksum_for '$BATS_TEST_TMPDIR/checksums.txt' mazet-v1.2.3-x86_64-unknown-linux-gnu.tar.gz.sig"
    [ "$status" -ne 0 ]
}

# --- sha256_of and verify_checksum ------------------------------------------

@test "sha256_of hashes a file to the value sha256sum reports" {
    printf 'mazet' >"$BATS_TEST_TMPDIR/payload"
    expected=$(sha256sum "$BATS_TEST_TMPDIR/payload" | cut -d' ' -f1)
    run sourced "sha256_of '$BATS_TEST_TMPDIR/payload'"
    [ "$status" -eq 0 ]
    [ "$output" = "$expected" ]
}

@test "verify_checksum accepts a file whose hash matches" {
    printf 'mazet' >"$BATS_TEST_TMPDIR/payload"
    expected=$(sha256sum "$BATS_TEST_TMPDIR/payload" | cut -d' ' -f1)
    run sourced "verify_checksum '$BATS_TEST_TMPDIR/payload' '$expected'"
    [ "$status" -eq 0 ]
}

@test "verify_checksum accepts an uppercase expectation" {
    printf 'mazet' >"$BATS_TEST_TMPDIR/payload"
    expected=$(sha256sum "$BATS_TEST_TMPDIR/payload" | cut -d' ' -f1 | tr 'a-f' 'A-F')
    run sourced "verify_checksum '$BATS_TEST_TMPDIR/payload' '$expected'"
    [ "$status" -eq 0 ]
}

@test "verify_checksum refuses a file whose hash does not match" {
    printf 'mazet' >"$BATS_TEST_TMPDIR/payload"
    run sourced "verify_checksum '$BATS_TEST_TMPDIR/payload' '$(printf '0%.0s' $(seq 64))'"
    [ "$status" -ne 0 ]
    [[ "$output" == *"mismatch"* ]]
    [[ "$output" == *"Nothing was installed"* ]]
}

# --- on_path ----------------------------------------------------------------

@test "on_path finds a directory that is a PATH component" {
    run sourced "PATH=/usr/bin:/home/me/.local/bin:/bin on_path /home/me/.local/bin"
    [ "$status" -eq 0 ]
}

@test "on_path does not accept a directory that is only a substring of one" {
    run sourced "PATH=/usr/bin:/home/me/.local/bin/extra:/bin on_path /home/me/.local/bin"
    [ "$status" -ne 0 ]
}

@test "on_path accepts a directory named with a trailing separator" {
    run sourced "PATH=/usr/bin:/home/me/.local/bin:/bin on_path /home/me/.local/bin/"
    [ "$status" -eq 0 ]
}

@test "on_path handles the first and last component" {
    run sourced "PATH=/home/me/.local/bin:/bin on_path /home/me/.local/bin"
    [ "$status" -eq 0 ]
    run sourced "PATH=/bin:/home/me/.local/bin on_path /home/me/.local/bin"
    [ "$status" -eq 0 ]
}

# --- install_binary ---------------------------------------------------------

# The release archive carries LICENSE and README.md beside the binary (see
# cd.yml's "Create archive (Unix)" step), so build the real thing.
release_archive() {
    mkdir -p "$BATS_TEST_TMPDIR/src"
    printf '#!/bin/sh\necho %s\n' "$1" >"$BATS_TEST_TMPDIR/src/mazet"
    printf 'MIT\n' >"$BATS_TEST_TMPDIR/src/LICENSE"
    printf '# mazet\n' >"$BATS_TEST_TMPDIR/src/README.md"
    tar -czf "$BATS_TEST_TMPDIR/archive.tar.gz" -C "$BATS_TEST_TMPDIR/src" mazet LICENSE README.md
}

@test "install_binary puts an executable mazet in the install directory" {
    release_archive first
    mkdir -p "$BATS_TEST_TMPDIR/stage"
    run sourced "install_binary '$BATS_TEST_TMPDIR/archive.tar.gz' '$BATS_TEST_TMPDIR/bin' '$BATS_TEST_TMPDIR/stage'"
    [ "$status" -eq 0 ]
    [ -x "$BATS_TEST_TMPDIR/bin/mazet" ]
    [ "$(sh "$BATS_TEST_TMPDIR/bin/mazet")" = "first" ]
}

@test "install_binary leaves LICENSE and README.md out of the bin directory" {
    release_archive first
    mkdir -p "$BATS_TEST_TMPDIR/stage"
    sourced "install_binary '$BATS_TEST_TMPDIR/archive.tar.gz' '$BATS_TEST_TMPDIR/bin' '$BATS_TEST_TMPDIR/stage'"
    [ ! -e "$BATS_TEST_TMPDIR/bin/LICENSE" ]
    [ ! -e "$BATS_TEST_TMPDIR/bin/README.md" ]
    run ls "$BATS_TEST_TMPDIR/bin"
    [ "$output" = "mazet" ]
}

@test "install_binary creates the install directory when it is absent" {
    release_archive first
    mkdir -p "$BATS_TEST_TMPDIR/stage"
    [ ! -d "$BATS_TEST_TMPDIR/deep/nested/bin" ]
    run sourced "install_binary '$BATS_TEST_TMPDIR/archive.tar.gz' '$BATS_TEST_TMPDIR/deep/nested/bin' '$BATS_TEST_TMPDIR/stage'"
    [ "$status" -eq 0 ]
    [ -x "$BATS_TEST_TMPDIR/deep/nested/bin/mazet" ]
}

@test "install_binary replaces an older binary and leaves nothing behind" {
    release_archive first
    mkdir -p "$BATS_TEST_TMPDIR/stage" "$BATS_TEST_TMPDIR/bin"
    sourced "install_binary '$BATS_TEST_TMPDIR/archive.tar.gz' '$BATS_TEST_TMPDIR/bin' '$BATS_TEST_TMPDIR/stage'"
    rm -rf "${BATS_TEST_TMPDIR:?}/src" "$BATS_TEST_TMPDIR/stage"
    release_archive second
    mkdir -p "$BATS_TEST_TMPDIR/stage"
    sourced "install_binary '$BATS_TEST_TMPDIR/archive.tar.gz' '$BATS_TEST_TMPDIR/bin' '$BATS_TEST_TMPDIR/stage'"
    [ "$(sh "$BATS_TEST_TMPDIR/bin/mazet")" = "second" ]
    run ls -A "$BATS_TEST_TMPDIR/bin"
    [ "$output" = "mazet" ]
}

@test "install_binary refuses an archive with no mazet in it" {
    mkdir -p "$BATS_TEST_TMPDIR/empty" "$BATS_TEST_TMPDIR/stage"
    printf 'MIT\n' >"$BATS_TEST_TMPDIR/empty/LICENSE"
    tar -czf "$BATS_TEST_TMPDIR/bad.tar.gz" -C "$BATS_TEST_TMPDIR/empty" LICENSE
    run sourced "install_binary '$BATS_TEST_TMPDIR/bad.tar.gz' '$BATS_TEST_TMPDIR/bin' '$BATS_TEST_TMPDIR/stage'"
    [ "$status" -ne 0 ]
    [ ! -e "$BATS_TEST_TMPDIR/bin/mazet" ]
}

# --- verify_runs ------------------------------------------------------------

# The failure this guards against arrives *after* a successful install: the
# dynamic linker refuses a binary whose glibc floor is above this machine's,
# and says so in its own words, naming neither mazet nor the installer.

@test "verify_runs accepts a binary that starts and reports its version" {
    printf '#!/bin/sh\necho "mazet 1.2.3"\n' >"$BATS_TEST_TMPDIR/works"
    chmod +x "$BATS_TEST_TMPDIR/works"
    run sourced "verify_runs '$BATS_TEST_TMPDIR/works' v1.2.3"
    [ "$status" -eq 0 ]
    [[ "$output" == *"mazet 1.2.3"* ]]
}

@test "verify_runs refuses a binary that cannot start, in its own words" {
    # What Debian 12 printed for the v0.1.0 gnu asset.
    printf '#!/bin/sh\n' >"$BATS_TEST_TMPDIR/broken"
    printf '%s\n' 'echo "mazet: /lib/x86_64-linux-gnu/libc.so.6: version \`GLIBC_2.39'"'"' not found (required by mazet)" >&2' \
        >>"$BATS_TEST_TMPDIR/broken"
    printf 'exit 1\n' >>"$BATS_TEST_TMPDIR/broken"
    chmod +x "$BATS_TEST_TMPDIR/broken"
    run sourced "verify_runs '$BATS_TEST_TMPDIR/broken' v1.2.3"
    [ "$status" -ne 0 ]
    # Its own explanation, not just the linker's line.
    [[ "$output" == *"does not run on this machine"* ]]
    # And the linker's line too, because that is the evidence for a bug report.
    [[ "$output" == *"GLIBC_2.39"* ]]
}

# main() reads the version out of a command substitution, and an `exit` inside
# one only leaves the subshell -- the trap this file's require_tools comment
# already describes. Under `set -eu` the failing assignment has to take the
# whole script down, not print an error and carry on to "is installed at".
@test "verify_runs failing inside a command substitution stops the script" {
    printf '#!/bin/sh\necho "linker said no" >&2\nexit 1\n' >"$BATS_TEST_TMPDIR/broken"
    chmod +x "$BATS_TEST_TMPDIR/broken"
    run sourced "
        reported=\$(verify_runs '$BATS_TEST_TMPDIR/broken' v1.2.3)
        echo \"KEPT GOING: \$reported\"
    "
    [ "$status" -ne 0 ]
    [[ "$output" != *"KEPT GOING"* ]]
}

@test "verify_runs refuses a file that is not executable at all" {
    printf 'not a binary\n' >"$BATS_TEST_TMPDIR/notexec"
    run sourced "verify_runs '$BATS_TEST_TMPDIR/notexec' v1.2.3"
    [ "$status" -ne 0 ]
}

# --- the whole download-verify-install path ---------------------------------

@test "a verified archive installs, and a tampered one does not" {
    release_archive first
    hash=$(sha256sum "$BATS_TEST_TMPDIR/archive.tar.gz" | cut -d' ' -f1)
    printf '%s  ./mazet-v1.2.3-x86_64-unknown-linux-gnu.tar.gz\n' "$hash" \
        >"$BATS_TEST_TMPDIR/sums.txt"
    mkdir -p "$BATS_TEST_TMPDIR/stage"

    run sourced "
        want=\$(checksum_for '$BATS_TEST_TMPDIR/sums.txt' mazet-v1.2.3-x86_64-unknown-linux-gnu.tar.gz)
        verify_checksum '$BATS_TEST_TMPDIR/archive.tar.gz' \"\$want\"
        install_binary '$BATS_TEST_TMPDIR/archive.tar.gz' '$BATS_TEST_TMPDIR/bin' '$BATS_TEST_TMPDIR/stage'
    "
    [ "$status" -eq 0 ]
    [ -x "$BATS_TEST_TMPDIR/bin/mazet" ]

    # Same checksum file, a different payload: the install must not happen.
    rm -rf "${BATS_TEST_TMPDIR:?}/src" "$BATS_TEST_TMPDIR/stage" "$BATS_TEST_TMPDIR/bin"
    release_archive tampered
    mkdir -p "$BATS_TEST_TMPDIR/stage"
    run sourced "
        want=\$(checksum_for '$BATS_TEST_TMPDIR/sums.txt' mazet-v1.2.3-x86_64-unknown-linux-gnu.tar.gz)
        verify_checksum '$BATS_TEST_TMPDIR/archive.tar.gz' \"\$want\"
        install_binary '$BATS_TEST_TMPDIR/archive.tar.gz' '$BATS_TEST_TMPDIR/bin' '$BATS_TEST_TMPDIR/stage'
    "
    [ "$status" -ne 0 ]
    [ ! -e "$BATS_TEST_TMPDIR/bin/mazet" ]
}

@test "an archive whose binary cannot start is not reported as installed" {
    # The v0.1.0 bug end to end: checksum good, extraction good, and the thing
    # still does not run. The installer has to be the one that says so.
    mkdir -p "$BATS_TEST_TMPDIR/src" "$BATS_TEST_TMPDIR/stage"
    printf '#!/bin/sh\necho "linker said no" >&2\nexit 1\n' >"$BATS_TEST_TMPDIR/src/mazet"
    printf 'MIT\n' >"$BATS_TEST_TMPDIR/src/LICENSE"
    printf '# mazet\n' >"$BATS_TEST_TMPDIR/src/README.md"
    tar -czf "$BATS_TEST_TMPDIR/broken.tar.gz" -C "$BATS_TEST_TMPDIR/src" mazet LICENSE README.md

    run sourced "
        install_binary '$BATS_TEST_TMPDIR/broken.tar.gz' '$BATS_TEST_TMPDIR/bin' '$BATS_TEST_TMPDIR/stage'
        verify_runs '$BATS_TEST_TMPDIR/bin/mazet' v1.2.3
    "
    [ "$status" -ne 0 ]
    [[ "$output" == *"does not run on this machine"* ]]
}
