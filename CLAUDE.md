# CLAUDE.md

Guidance for a coding agent working in this repository. It is an index into the
documents that own the reasoning, plus the few things that are true on every
turn.

## What this crate is

`mazet` runs `az` under several Azure identities at once, chosen by the
directory you are standing in. The Azure CLI keeps every credential, token
cache and its `azureProfile.json` under one directory named by
`AZURE_CONFIG_DIR`, so two logins into it compete and one store holds exactly
one active subscription. `mazet` decides **which** directory a given command
should use and hands it to `az` in the child's environment. That is the whole
mechanism.

## Where the facts live

| question | document |
|---|---|
| why the code is arranged this way, and what the layering is | [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) |
| why a feature behaves the way it does | [`docs/FEATURES.md`](docs/FEATURES.md) |
| what a key, a variable or a path is | [`docs/CONFIG.md`](docs/CONFIG.md) |
| how to build, test and lint | [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) |
| how a release happens | [`docs/RELEASING.md`](docs/RELEASING.md) |
| how to propose a change | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
| what the tool does, for a user | [`README.md`](README.md) |

**A code change that invalidates or extends a documented decision updates the
owning document in the same change.** Each class of fact has exactly one owner;
if you find the same fact in two places, make one of them a pointer rather than
updating both.

## Rules that hold on every turn

- **No key of any `mazet` file may hold a credential.** A tenant, a
  subscription, a cloud, a username and a client id are identifiers and belong
  in a committed config. The parser refuses anything that authenticates.
  Secrets reach `az` from the environment at login time, always as a path,
  never through argv.
- **`src/az.rs` is the only module that starts a process or touches a
  credential**, and it depends on nothing else in the crate.
  `tests/architecture_rules.rs` enforces both halves. A `Command::new` anywhere
  else fails the suite, and so does a new module with no rules entry.
- **Nothing under `src/` mutates this process's environment.**
  `AZURE_CONFIG_DIR` is set on the child; `mazet env` prints code for a shell to
  evaluate.
- **Do not weaken an allowlist entry to make the architecture test pass.**
  Widening one is a decision — record it in `docs/ARCHITECTURE.md` in the same
  pull request.
- **A behaviour is asserted by running the code**, never by grepping the source
  for a string. Tests live under `tests/`, one file per surface; there are no
  `#[cfg(test)]` blocks under `src/`.
- **A bug fix starts with a test that reproduces it** — failing before the fix,
  passing after.
- **Comments explain why, never what.** No TODO, FIXME or HACK markers, and no
  commented-out code.
- **Every error says what to do next.** One line naming the failure, then
  indented lines naming the fix; `cli::error_output` splits on that boundary to
  render `error` and `suggestion` separately.

## The gate

`prek run --all-files`, before you push. It is what CI runs. Details and the
per-hook breakdown are in [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md#the-gate).

The remote takes **squash merges only**, so the pull request title becomes the
commit on `main` — and its conventional-commit type is what decides the version
the release workflow then cuts.
