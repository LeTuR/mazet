#!/usr/bin/env bash
#
# Verify a range's commit messages against cog.toml's conventional-commit rules.
#
# `cog check` does this in one call, and is what this would be if the
# no-mistakes gate did not author commits of its own. It commits the fixes each
# of its phases writes under a subject that binary templates itself —
# `no-mistakes: <summary>` or `no-mistakes(<phase>): <summary>` — which is not a
# conventional commit and which no repository setting can retemplate. `cog
# check` cannot exempt a single commit, so the commit pushed to fix this check
# would fail it, and no further fix could land.
#
# Every other commit is held to exactly what `cog check` enforced: `cog verify`
# reads the same cog.toml, so the type and scope allowlists still apply, and
# merge commits are skipped the way `ignore_merge_commits` skipped them.
set -euo pipefail

# Commits the no-mistakes gate authors itself. Only the subject's type — and
# its optional scope, which is the phase that wrote the fix — is fixed; the
# summary after it varies per fix. Anchored at the start, so a hand-written
# message that merely mentions the gate is still checked.
GATE_SUBJECT='^no-mistakes(\([^)]+\))?: '

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/borrowed-identity.sh
. "$here/borrowed-identity.sh"

# Default to every commit reachable from HEAD: this repository's history starts
# at its first commit, not at a tag.
range="${1:-HEAD}"

non_compliant=0

while IFS= read -r sha; do
    subject=$(git log -1 --format=%s "$sha")
    if [[ "$subject" =~ $GATE_SUBJECT ]]; then
        continue
    fi

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
