# Contributing to mazet

Thanks for wanting to help. This is a small crate with a strong opinion about
one thing — that `az` should be able to hold several identities at once — so
most changes are small, and the checks are what keep them safe.

Be respectful and constructive: assume good faith, keep discussions on the
technical merits, and help newcomers find their footing.

## Proposing a change

1. **Open an issue first** if the change is more than a fix — the shape of the
   `.mazet` format and the store derivation are load-bearing, and a design
   conversation is cheaper than a rewritten pull request.
2. **Branch off `main`** (`git switch -c feat/my-change`). Push directly if you
   have write access, otherwise fork first.
3. **Make the change, with tests.** A behaviour is asserted by running the
   code, never by grepping the source for a string.
4. **Run the gate** (below) and make it green.
5. **Open a pull request** against `main` with a conventional-commit title.

## The toolchain and the gate

**`prek run --all-files` is this repository's gate.** Run it before you push;
it is the same set of checks CI runs, and it is what the `no-mistakes` pipeline
runs too.

[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) has the rest and is where those
facts live: the toolchain and the MSRV, the binaries to install first, what
each hook runs, how the test suite is laid out, and the architecture test that
decides what a module may reference.

## The rules the crate is held to

[`CLAUDE.md`](CLAUDE.md) states them in one place — it is written for a coding
agent, but the rules are the same ones a reviewer applies here, and the two
that are not style preferences have their own section below.

## Commit conventions

Commits are [conventional commits](https://www.conventionalcommits.org/),
enforced by `cog` against [`cog.toml`](cog.toml) on the `commit-msg` hook and
again in CI.

```text
<type>(<scope>): <subject>
```

Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `style`, `test`, `chore`,
`build`, `ci`, `revert`. Scopes: `cli`, `config`, `store`, `az`, `docs`,
`deps`, `ci`.

The **pull request title** is held to the same rules by its own workflow,
because under squash merges it is the commit that lands — see
[Pull requests](#pull-requests).

```text
feat(config): accept a .mazet directory as well as a file
fix(store): re-assert 0700 on a store whose mode drifted
```

## Pull requests

**The remote takes squash merges only** — rebase and merge commits are both
disabled on `github.com/LeTuR/mazet`. So:

- **the pull request title becomes the commit on `main`**, and nothing else on
  the branch does. Write it as the one-line history entry it will be — and note
  that its type is what decides the version the release workflow then cuts;
- keep your branch rebased on `main` anyway (`git pull --rebase`, and
  `pull.rebase` is worth setting to `true` locally), so what CI reviews is what
  would land;
- do not merge `main` into your branch.

The commits on the branch are still held to `cog.toml` — they are what a
reviewer reads — but they are not what history keeps.

The branch is deleted on merge.

## Releasing

**Nobody cuts a release.** A `feat` or a `fix` merged to `main` becomes a tag
and a published GitHub Release with no human action:
[`release.yml`](.github/workflows/release.yml) asks `cog` whether a version is
due, tags it, and starts [`cd.yml`](.github/workflows/cd.yml), which
cross-compiles the five release targets, checksums the archives and publishes
the release.

So the pull request title you write is the version you cut — `feat` a minor,
`fix` and `perf` a patch, everything else nothing at all. A merge titled
`docs`, `chore`, `ci` or `refactor` finishes green and tags nothing.

To land a change without releasing it, put `[skip release]` in that title.

[`docs/RELEASING.md`](docs/RELEASING.md) has the rest: what triggers what and
why the tag push alone is not enough, the five targets and their asset names,
how to re-run a release whose build failed, and how the installers consume it.

## What not to put in this crate

Two rules that are not style preferences:

- **No key of any `mazet` file may hold a credential.** A tenant, a
  subscription, a cloud, a username and a client id are identifiers and belong
  in a committed config. A password, a client secret, a certificate or a token
  does not, and the parser refuses one rather than storing it. `mazet` holds
  paths and names; `az` holds secrets, in the directory `mazet` points it at.
- **A store directory never defaults into the repository being worked on, nor
  into `/tmp`.** It is `0700` on Unix, and `mazet` writes the ignore rules that
  keep it and the local override uncommittable.

## License

By contributing you agree that your contributions are licensed under the MIT
license, the same as the rest of the project.
