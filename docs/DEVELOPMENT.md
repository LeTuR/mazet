# Development

Building, testing and the gates. How to *propose* a change — the commit
conventions, the pull-request rules — is
[`CONTRIBUTING.md`](../CONTRIBUTING.md)'s; releasing is
[`RELEASING.md`](RELEASING.md)'s.

## The toolchain

Rust stable, from [`rust-toolchain.toml`](../rust-toolchain.toml) — `rustup`
picks it up on its own.

**MSRV is 1.85, Edition 2021**, declared in three places that have to agree:
[`Cargo.toml`](../Cargo.toml) (`rust-version`, what cargo enforces),
[`clippy.toml`](../clippy.toml) (`msrv`, what clippy lints against) and
[`CONTRIBUTING.md`](../CONTRIBUTING.md) (what a contributor reads). The pinned
toolchain is deliberately *stable* and not the MSRV: the floor is what a
consumer needs, and gating against it would mean losing every lint and
diagnostic newer than it.

Two extra binaries:

```sh
cargo install prek       # the hook runner
cargo install cocogitto  # `cog`, the commit-message checker
prek install             # wire the git hooks, once
```

Two more if you touch the installers, from your package manager
(`apt install shellcheck bats`, `brew install shellcheck bats-core`):
`shellcheck` and `bats`. The `install.ps1` half of that pair is tested with
[Pester](https://pester.dev) and runs in CI only, since `pwsh` is not something
this repository asks you to install.

## The gate

**`prek run --all-files` is this repository's gate.** It is the same set of
checks CI runs and what the `no-mistakes` pipeline runs, and the hooks are all
`language: system` so each one executes the same binary CI does rather than a
copy `prek` built for itself.

| | what it runs |
|---|---|
| format | `cargo fmt --all -- --check` |
| lint | `cargo clippy --all-targets --all-features -- -D warnings` |
| compile | `cargo check --all-targets --all-features` |
| test | `cargo test --all-features` |
| docs | `RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --all-features` |
| shell | `shellcheck` on every tracked `*.sh` |
| installer | `bats tests/install.bats` |
| commits | `cog verify` on the message, and the history on pre-push |

`cargo fmt --all` fixes the first one for you.

The rustdoc check is part of the gate and not an afterthought: `src/lib.rs`
carries `#![deny(missing_docs)]`, the crate documents itself in intra-doc
links, and a broken one is a CI failure like any other.

The suite runs on Linux, macOS and Windows in CI, because the store layout and
its permissions differ per platform and a Linux-only run would assert the other
two only by hope.

## The test suite

Integration tests, one file per surface, all under `tests/`. There are no
`#[cfg(test)]` blocks under `src/` — every module is a file, and every test
drives a real interface.

| file | what it covers |
|---|---|
| `architecture_rules.rs` | the module layering, as an allowlist — below |
| `config.rs` | parsing a `.mazet`: both spellings, and the errors |
| `layering.rs` | the two layers and the store-resolution precedence |
| `degradation.rs` | an absent field is never an error; a malformed one always is |
| `discovery.rs` | the walk up to the `.mazet` that applies |
| `environments.rs` | environments are stores, not a switch |
| `profiles.rs` | the registry and the store directories it names |
| `init.rs`, `which.rs`, `hook.rs` | those commands, end to end |
| `login.rs`, `logout.rs`, `exec.rs`, `env.rs`, `status.rs` | the `az` surface |
| `az.rs` | finding `az`, and the `MAZET_AZ` override |
| `cli.rs` | the binary, driven the way an operator and an agent each drive it |
| `install.bats`, `install.Tests.ps1` | the two installers |

`tests/common/mod.rs` gives every test its own temporary tree with its own data
and config roots, so nothing reaches the developer's real
`~/.local/share/mazet` and tests run in parallel.

### The `az` stub

[`examples/az_stub.rs`](../examples/az_stub.rs) stands in for the Azure CLI.
It records every invocation — argv, the environment variables that decide an
identity, and the contents of any `@<file>` argument — as one JSON line, so a
test can assert exactly what `mazet` would have handed the real `az` without a
tenant, a network, or a credential anywhere near it. It is also a believable
`az` in the small: `login` writes an `azureProfile.json`, `logout` empties its
account list, and `account show` prints it back. Its knobs are documented in
its own header.

A behaviour is asserted by running the code. A test whose only evidence is that
it greps the source for a string proves nothing — the text can be dead, and a
behaviour-preserving refactor changes it.

### The architecture test

[`tests/architecture_rules.rs`](../tests/architecture_rules.rs) is an
**allowlist**: every module under `src/` must have a `MODULE_RULES` entry (or
sit in `EXEMPT`) naming the crate modules it may reference, and
`every_module_is_governed` fails when one does not. Three more tests hold the
rules an allowlist cannot express — `az` is the only module that starts a
process, and nothing mutates this process's environment.

Adding a module means adding its entry. Adding a dependency between two modules
means widening an entry, and that is the decision the file exists to make
visible: **a boundary is not weakened to make the test pass.** If the layering
is changing on purpose, [`ARCHITECTURE.md`](ARCHITECTURE.md) changes in the same
pull request — the failure message says so too.

Run it on its own with:

```sh
cargo test --test architecture_rules
```

## Running one thing

```sh
cargo test --all-features                    # the suite
cargo test --test login                      # one file
cargo test --test login -- --nocapture name  # one test, with its output
cargo run -- which                           # the binary, in this directory
MAZET_DATA_DIR=/tmp/d MAZET_CONFIG_DIR=/tmp/c cargo run -- profile list
```

Those last two variables are the safe way to drive a real build by hand: they
move both roots out of your own, so nothing you try lands beside your real
stores. See [`CONFIG.md`](CONFIG.md#other-variables-mazet-reads).

## Dependencies

Kept few on purpose, and each one is there for a reason recorded beside it in
[`Cargo.toml`](../Cargo.toml). `toon-format` is pulled with
`default-features = false` because its defaults bring a whole TUI viewer this
crate has no use for. [Renovate](../renovate.json) proposes updates.
