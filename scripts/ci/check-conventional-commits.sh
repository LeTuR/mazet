#!/usr/bin/env bash
#
# Verify a range's commit messages against cog.toml's conventional-commit rules.
#
# `cog check` does this in one call, and is what this would be if the
# no-mistakes gate did not author commits of its own. It commits the fixes its
# CI step writes under a subject baked into that binary — `no-mistakes: apply
# CI fixes` — which is not a conventional commit and which no repository
# setting can retemplate. `cog check` cannot exempt a single commit, so the
# commit pushed to fix this check would fail it, and no further fix could land.
#
# Every other commit is held to exactly what `cog check` enforced: `cog verify`
# reads the same cog.toml, so the type and scope allowlists still apply, and
# merge commits are skipped the way `ignore_merge_commits` skipped them.
set -euo pipefail

# Subjects the no-mistakes binary hardcodes for commits it authors itself.
# Matched whole, so a hand-written message that merely mentions one is checked.
GATE_SUBJECTS=(
    "no-mistakes: apply CI fixes"
    "no-mistakes: apply agent fixes"
)

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/borrowed-identity.sh
. "$here/borrowed-identity.sh"

# Default to every commit reachable from HEAD: this repository's history starts
# at its first commit, not at a tag.
range="${1:-HEAD}"

non_compliant=0

while IFS= read -r sha; do
    subject=$(git log -1 --format=%s "$sha")
    for gate_subject in "${GATE_SUBJECTS[@]}"; do
        if [ "$subject" = "$gate_subject" ]; then
            continue 2
        fi
    done

    if ! report=$(git log -1 --format=%B "$sha" | cog verify --file - 2>&1); then
        printf 'Errored commit: %s\n\tCommit message: %s\n%s\n' \
            "$sha" "$subject" "$report" >&2
        non_compliant=$((non_compliant + 1))
    fi
done < <(git rev-list --no-merges "$range")

if [ "$non_compliant" -ne 0 ]; then
    printf 'Found %d non compliant commits in %s\n' "$non_compliant" "$range" >&2
    exit 1
fi

printf 'No errored commits in %s\n' "$range"
