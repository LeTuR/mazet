#!/usr/bin/env bash
#
# Verify a pull request title is a conventional commit.
#
# The remote takes squash merges only, so the PR title IS the commit that lands
# on main: it is what the release notes quote, what a reviewer reads first, and
# what release.yml's `cog bump --auto` reads to decide the next version.
# Nothing else checks it: the sibling script walks commits, not titles.
#
# Usage: check-pr-title.sh <title>
set -euo pipefail

if [ "$#" -ne 1 ]; then
    printf 'usage: %s <title>\n' "${0##*/}" >&2
    exit 2
fi

title="$1"

if [ -z "$title" ]; then
    printf 'The pull request title is empty.\n' >&2
    exit 1
fi

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/borrowed-identity.sh
. "$here/borrowed-identity.sh"

# cog.toml is read from the working directory, so the type and scope allowlists
# this repository declares are part of what is enforced here.
if ! report=$(printf '%s\n' "$title" | cog verify --file - 2>&1); then
    printf 'The pull request title is not a conventional commit.\n\n' >&2
    printf '  title: %s\n\n' "$title" >&2
    printf '%s\n\n' "$report" >&2
    printf 'Valid types and scopes are declared in cog.toml.\n' >&2
    exit 1
fi

printf 'Pull request title is a conventional commit: %s\n' "$title"
