# Architecture

Why `mazet` is shaped the way it is, and the layering
[`tests/architecture_rules.rs`](../tests/architecture_rules.rs) enforces.

This document owns the decisions and their rationale. The feature-level
choices they enable are [`FEATURES.md`](FEATURES.md)'s, every key and variable
is [`CONFIG.md`](CONFIG.md)'s, and how to use the tool is the
[root README](../README.md)'s.

## The constraint everything follows from

The Azure CLI keeps **all** of its per-identity state in one directory, named
by `AZURE_CONFIG_DIR` and defaulting to `~/.azure`: the token cache, the
service-principal entries, the cloud chosen by `az cloud set`, and
`azureProfile.json` — the subscription list, with exactly one subscription
marked `isDefault`.

Two consequences, and the whole tool is downstream of them:

- **Two logins into one directory compete.** The second displaces the first, so
  a service-principal login can silently take over from a user one.
- **One store holds exactly one active subscription.** `az account set -s`
  moves the `isDefault` mark, so a `terraform apply` against prod and an `az`
  query against dev out of one store fight over which is active.

`mazet` therefore does not switch anything. It **decides which directory a
given command should use** and hands that directory to `az` in the child's
environment. That is the entire mechanism.

It is also why **an environment is a store rather than a switch**. A `.mazet`
declaring `[env.dev]` and `[env.prod]` resolves each to its own
`AZURE_CONFIG_DIR`, because the alternative — one store and an `az account set`
between uses — is the interference this tool exists to remove. The subscription
is part of the derived store key for exactly this reason
([`CONFIG.md` § store-key derivation](CONFIG.md#the-derived-store-key)).

## Driving `az`, not replacing it

`mazet` shells out to the Azure CLI. It holds no Entra client, speaks no OAuth,
and links no Azure SDK.

The reason is that the artifact being produced is `az`'s own on-disk state, in
`az`'s own formats, for `az` and every other tool that reads
`AZURE_CONFIG_DIR` — `terraform`, `kubelogin`, the Azure SDKs. A reimplemented
login would have to write that state itself and would be wrong the next time
`az` changed it. Letting `az` write its own directory means `mazet` never has
to know what is in one.

The cost is accepted deliberately: `az` must be installed, `mazet` inherits its
flags, and finding it on Windows takes real work because the Azure CLI installs
as `az.cmd`, a batch script that `CreateProcess` will not resolve from a bare
name — so [`az::Az::discover`](../src/az.rs) walks `PATH` itself and tries each
`PATHEXT` suffix.

## The layering

```text
                     ┌──────────────────────────────┐
   the command line  │ cli::{login,logout,exec,…}_cmd│  argv in, document out
                     │ cli::select   cli::output     │
                     └──────┬────────────────┬───────┘
                            │                │
      plan, do not run      │                │
                     ┌──────┴───────┐        │
                     │ login   exec │        │
                     └──────┬───────┘        │
                            │                │
   parse and derive  ┌──────┴────────────────┴───────┐
      (pure)         │ config  discover  resolve     │
                     │ paths   profile   store       │
                     │ init    explain               │
                     └───────────────────────────────┘

   leaves, reaching nothing:   az     hook     status
```

Read top to bottom: everything above may reach what is below it, and nothing
reaches back up. The three leaves sit outside that stack because they depend on
no other module in the crate — `hook` emits shell source, `status` parses `az
account show` output, and `az` is the subject of the next section.

Each module's exact allowlist is
[`MODULE_RULES`](../tests/architecture_rules.rs), which is the authority; the
table below is what it means.

| module | what it is |
|---|---|
| `az` | the only code that starts a process or touches a credential |
| `config` | the `.mazet` format: both spellings, both layers, validation |
| `discover` | walking up from a directory to the `.mazet` that applies |
| `paths` | where `mazet` keeps things, per platform |
| `profile` | the central registry: named profiles, per-tenant identity defaults |
| `store` | creating a store directory, and keeping it out of git |
| `resolve` | a parsed config plus a selection → one `AZURE_CONFIG_DIR` |
| `init` | writing a `.mazet` |
| `explain` | where each effective value came from, for `mazet which` |
| `hook` | the shell source for bash, zsh, fish and PowerShell |
| `login` | the ordered `az` invocations a login would be, in every mode |
| `exec` | the variables a child process is given, or stripped of |
| `status` | parsing `az account show` into an account |
| `cli` | the subcommand tree, the shared context, and one module per command |

`cli`'s submodules are governed individually rather than as one `cli` entry.
They are half the crate, and the boundary that matters runs between them.

## The boundary that would hurt to break

**`src/az.rs` is the only module that starts a process or touches a
credential, and it reaches nothing.**

Both halves of that are load-bearing, and each is enforced by a different test.

The allowlist holds one half: `az`'s entry is `allowed: &[]`, so the module
that spawns children and writes secret material cannot read a `.mazet`, derive
a store path or consult the registry. The code a reviewer has to audit for
credential handling is one file and its callers, not the crate.

The other half cannot be expressed as an allowlist entry at all — an entry
records that `login` references `az`, not that `login` stopped short of
spawning — so `only_az_starts_a_process` scans every file under `src/` for a
`Command::new` outside `src/az.rs`. **A `.mazet` is a file a repository
commits.** A config parser or a path helper that could spawn would turn a
committed file into something that executes, which is the one outcome this
crate exists to make impossible, and the parse-and-derive layer has no entry
naming `az` or `exec` precisely so that it cannot grow one by import either.

A third test, `nothing_mutates_this_process_environment`, holds the rule that
makes `mazet exec` safe to put in a pipeline: `AZURE_CONFIG_DIR` is set on the
**child**, never on `mazet` itself. `std::env::set_var` appears nowhere under
`src/`, so no command can move the calling shell's identity or write to the
operator's own `~/.azure`. `mazet env` prints assignments for a shell to
evaluate, and that is the difference.

## Planning apart from running

`login` decides the ordered list of `az` invocations a login would be; `az`
runs them. `exec` decides which variables a child is given or stripped of; `az`
applies them. Neither planning module spawns anything.

The split is what makes *"which argv would this produce?"* a question a test
can ask without an Azure tenant on the other end — and
[`tests/login.rs`](../tests/login.rs) asks it for every authentication mode.
The alternative, a function that builds and spawns in one pass, is testable
only against a real `az` and a real directory, which means in practice it is
tested against neither.

It also puts the order in one readable place, and the order is not cosmetic:

```text
  az cloud set -n <cloud>     first — it writes into AZURE_CONFIG_DIR, so a
                              login made before it authenticated against the
                              wrong cloud's endpoints
  az login <mode flags>       the authentication itself
  az account set -s <sub>     last — the subscription list does not exist
                              until a login discovered it
```

## Credentials

`mazet` files hold identifiers only. A tenant, a subscription, a cloud, a
username and a client id are identifiers and belong in a committed config;
anything that *authenticates* does not, and
[`config::Config::load`](../src/config.rs) refuses a key whose name reads like
a credential rather than storing it — checked against the raw TOML before
deserialization, so the refusal names the offending key and so a *future* key
of this crate's own can never quietly become a place to put a secret.

Secrets reach `az` from the environment at login time
([`CONFIG.md` § credentials](CONFIG.md#credentials-read-from-the-environment)),
under three rules that live in `az`:

- **A secret never reaches argv.** `az` expands an argument written `@<path>`
  by reading that file, so `az::Credentials` hands it a *path* for every
  credential, writing a `0600` file inside the `0700` store first when the
  secret arrived as a value and deleting it when the login is over. Argv is
  world-readable through `ps`; that file is not.
- **A credential is never rendered.** `az::Credential` has no accessor for the
  bytes and a `Debug` that prints the environment variable's *name*. An error
  from there names the variable to set, never what was in it.
- **A store is private and uncommittable.** `0700` on Unix, and `store` writes
  the ignore rules beside a config the moment `mazet` materialises anything
  there, before any store exists.

A store directory therefore never defaults into the repository being worked on,
nor into `/tmp`: the first makes a credential store committable, the second
makes it world-readable and short-lived. Both roots come from the `directories`
crate or from an explicit override, and neither default is either of those.

## The derived store name is a hash mazet owns

A config that names no profile and asks for no local store resolves to
`<data>/stores/<hint>-<16 hex digits>`, where the digits are FNV-1a over a
canonical, versioned string built from the effective identity. The algorithm is
implemented in [`resolve`](../src/resolve.rs) rather than taken from
`std::hash`, because `DefaultHasher` makes no promise of stability across Rust
releases — **and a store name that changes under an operator is a silent second
login.** The `v1` prefix on the canonical string is what makes a future change
to what contributes to the key an explicit migration rather than an accident.

The hint before the digits carries no meaning the key depends on. It is there
so a human reading the stores directory can tell one from another.

## Output is a document, and errors are on stdout

`mazet` is an [AXI](https://axi.md) (`axi/1.0-2026-07`), an interface shaped for
an agent as much as for a person: human-readable on a terminal, TOON down a
pipe, worked examples on every help surface, and errors as a structured
`error`/`suggestion` document on **stdout** with the exit code saying which kind
it is. A caller never has to read two streams to find the answer.

Two commands are deliberately exempt, and both for the same reason — the
consumer is a shell, not a reader. `mazet hook` and `mazet env` stay raw down a
pipe, because TOON there is something no shell can run; `mazet exec` prints no
document at all and passes the child's streams and exit status through
untouched. See [`FEATURES.md`](FEATURES.md#exec-is-transparent) for what that
buys.

## What the architecture test does not cover

Worth knowing before trusting it:

- **It reads text, not the compiler's resolution.** A module reached through a
  re-export (`pub use`) is credited to the module that re-exported it. Nothing
  in the crate does this today.
- **It sees an edge, not what crosses it.** `login` may reference `az`; whether
  it spawns is `only_az_starts_a_process`'s question, not the allowlist's.
- **Unqualified sibling calls inside `src/cli/` are invisible to it.** A
  reference is recorded when it is written `crate::…` or `super::…`, which is
  how those modules do reach each other today.
- **The three pure modules `config`, `profile` and `store` are mutually
  recursive** — `config` reads `profile` for `ProfileName`, `profile` reaches
  `store` to create the registry's directory, `store` reads `config` for
  `ConfigLocation`. They are one layer, not three, and the allowlist records
  that rather than pretending to an ordering the code does not have.
