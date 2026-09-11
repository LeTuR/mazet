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

## The toolchain

Rust stable, from [`rust-toolchain.toml`](rust-toolchain.toml) — `rustup` picks
it up on its own. **MSRV is 1.85, Edition 2021**, declared in
[`Cargo.toml`](Cargo.toml) and [`clippy.toml`](clippy.toml); keep the three in
step if you raise it.

Two extra binaries, both `cargo install`-able:

```sh
cargo install prek     # the hook runner
cargo install cocogitto # `cog`, the commit-message checker
prek install           # wire the git hooks, once
```

## The gate

**`prek run --all-files` is this repository's gate.** Run it before you push;
it is the same set of checks CI runs, and it is what the `no-mistakes` pipeline
runs too.

| | what it runs |
|---|---|
| format | `cargo fmt --all -- --check` |
| lint | `cargo clippy --all-targets --all-features -- -D warnings` |
| compile | `cargo check --all-targets --all-features` |
| test | `cargo test --all-features` |
| docs | `RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --all-features` |
| commits | `cog verify` on the message, and the history on pre-push |

`cargo fmt --all` fixes the first one for you.

The suite runs on Linux, macOS and Windows in CI, because the store layout and
its permissions differ per platform and a Linux-only run would assert the other
two only by hope.

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

The **pull request title** is held to the same rules by its own workflow, since
it is what the release notes quote.

```text
feat(config): accept a .mazet directory as well as a file
fix(store): re-assert 0700 on a store whose mode drifted
```

## Pull requests

**The remote takes rebase merges only** — merge commits and squash merges are
both disabled on `github.com/LeTuR/mazet`. So:

- keep your branch rebased on `main` (`git pull --rebase`, and `pull.rebase` is
  worth setting to `true` locally);
- every commit on your branch lands on `main` as written, so every one of them
  must be a well-formed conventional commit and should build on its own;
- do not merge `main` into your branch.

The branch is deleted on merge.

## Releasing

A release is cut from a green `main`:

```sh
cog bump --auto          # decides the version from the commits, tags it
git push --follow-tags
```

The tag is what starts [`cd.yml`](.github/workflows/cd.yml), which
cross-compiles the five release targets, checksums the archives and publishes
the GitHub Release.

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
