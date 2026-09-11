#!/usr/bin/env bash
#
# Lend git an author identity when the checkout has none.
#
# `cog verify` resolves the current git author before it parses anything and
# panics when `user.name` is unset — which is every fresh CI checkout, where
# nothing commits and so nothing configures an identity. The author is only
# printed back, never part of the verdict, so lend one through a throwaway HOME
# (libgit2 reads `$HOME/.gitconfig`) rather than writing into the repository
# being checked.
#
# Source this, do not execute it: it exports HOME into the caller's shell.
# shellcheck shell=bash

if ! git config --get user.name >/dev/null 2>&1 ||
    ! git config --get user.email >/dev/null 2>&1; then
    _mazet_borrowed_home=$(mktemp -d)
    # shellcheck disable=SC2064  # expand the path now, not at trap time
    trap "rm -rf '$_mazet_borrowed_home'" EXIT
    printf '[user]\n\tname = conventional commit checker\n\temail = checker@invalid\n' \
        >"$_mazet_borrowed_home/.gitconfig"
    export HOME="$_mazet_borrowed_home"
fi
