# Configuration reference

Every file `mazet` reads or writes, every key in them, every environment
variable it reads or hands to a child, and where things live on each platform.

This is the complete list and the authority where it and another document
touch. The [root README](../README.md) shows the same format as a tour, with
the worked examples; *why* a key works this way is
[`FEATURES.md`](FEATURES.md)'s.

## The `.mazet`, in both spellings

A directory tree declares its binding with a `.mazet` at its root, written
either way:

```text
.mazet                 # a FILE: the shared TOML, on its own
.mazet.local           #   ...and beside it, the local override

.mazet/                # a DIRECTORY
.mazet/config.toml     #   the same shared TOML
.mazet/local.toml      #   the local override
.mazet/store/          #   this folder's own AZURE_CONFIG_DIR
.mazet/.gitignore      #   written by mazet: covers store/ and local.toml
```

Both put the `.mazet` at the same path. The nearest one above the current
directory wins, and the walk stops at the filesystem root; the local layer is
never searched for separately, since it lives beside whichever config was
found. A `.mazet/` directory with no `config.toml` is an empty shared layer,
not an error.

A `.mazet` that is a symbolic link resolves to whatever it points at.

## Layer 1 — the shared config

`.mazet`, or `.mazet/config.toml`. Meant to be committed.

| key | type | meaning |
|---|---|---|
| `tenant` | GUID or DNS domain | which Entra tenant |
| `subscription` | GUID or display name | selected after login, with `az account set -s` |
| `cloud` | one of the five below | the registered `az` cloud |
| `method` | one of the five below | the default authentication method |
| `default_env` | name of an `[env.*]` block | which environment applies when nothing else chooses |
| `[env.<name>]` | table | an environment; may repeat `tenant`, `subscription`, `cloud`, `method` |

`cloud` is one of `AzureCloud`, `AzureChinaCloud`, `AzureUSGovernment`,
`AzureGermanCloud`, `AzureBleuCloud` — the names `az cloud list` prints.
Matching is case-insensitive and the value is canonicalized to that spelling.

`method` is one of `interactive`, `device-code`, `service-principal`,
`federated`, `managed-identity`. It is a *choice of which `az login` to make*,
never a credential.

An `[env.<name>]` block accepts only those four keys. A bare key written after
an `[env.*]` header belongs to that table in TOML, so `default_env` has to go
above the blocks — which is where `mazet init` writes it.

## Layer 2 — the local override

`.mazet.local`, or `.mazet/local.toml`. Never committed; `mazet init` writes
the ignore rule for it.

| key | type | meaning |
|---|---|---|
| `username` | string | this operator's user principal name in the tenant |
| `client_id` | string | their service principal or managed identity |
| `method` | one of the five | overrides the shared default |
| `store` | `local` \| `central` | `local` = `.mazet/store/`; `central` = a derived store under the data root |
| `profile` | registered profile name | use that profile's store instead; beats `store` |

`store = "local"` requires the `.mazet/` **directory** spelling — a flat
`.mazet` has nowhere to put a store — and the error says how to convert.

These five are local-layer keys. Written in the shared layer they still take
effect, and each produces a warning naming the key and the file it belongs in.

## Layering and precedence

**Per key, highest first:**

```text
  the local layer
    └─ the selected [env.<name>] block
         └─ the top-level shared keys
              └─ the default
```

with two departures from plain key-by-key merging:

- **`store` and `profile` are taken together**, from whichever layer picks a
  store. A local file spelling either one replaces both.
- **The identity has a third source.** `username` and `client_id` are resolved
  as **local layer → the registry's per-tenant default → shared layer**, the
  registry default applying only when the local layer named nobody and the
  effective `tenant` matches.

**Environment selection, highest first:**

| | source |
|---|---|
| 1 | the `--env <name>` flag |
| 2 | `MAZET_ENV` |
| 3 | `default_env` in the shared config |
| 4 | the sole `[env.*]` block, when the config declares exactly one |
| 5 | the top-level keys alone |

An `--env`, `MAZET_ENV` or `default_env` naming an environment the config does
not declare is an error listing the ones it does. Reaching 5 with several
blocks declared raises a warning naming them.

## What an absent key means

| absent | what happens |
|---|---|
| `tenant` | no `--tenant` on the login |
| `subscription` | nothing is selected after login; `az`'s own default stands |
| `cloud` | no `az cloud set` at all, so the store keeps the cloud it had; the effective value reported is `AzureCloud` |
| `method` | `interactive` |
| `default_env`, with several `[env.*]` and no selection | the top-level keys alone, and a warning naming the environments |
| every key | the store is bound to this config's location and nothing else |

A key that is *present but malformed* is always an error naming the file and
the key.

## Letter case

Entra treats a tenant, a subscription GUID and an identity case-insensitively,
so `mazet` folds them: writing one in two cases must not derive two stores.

| value | folding |
|---|---|
| `tenant` | trimmed, lowercased (both the GUID and the domain spelling) |
| `subscription` written as a GUID | trimmed, lowercased |
| `subscription` written as a display name | **kept exactly as written** — `az account set -s` matches it literally |
| `username`, `client_id` | trimmed, lowercased, in a `.mazet` and in the registry alike |
| `cloud`, `method` | matched case-insensitively, canonicalized to the spelling above |

## Keys that are refused

**No key of any `mazet` file may hold a credential.** A key whose name contains
any of `secret`, `password`, `passwd`, `token`, `credential`, `certificate`,
`privatekey`, `thumbprint`, `pfx`, `pem`, `connectionstring` or `sas` — after
dropping non-alphanumerics and lowercasing — is refused by name, at every depth
of the document, before deserialization.

Every other unknown key is refused too: both layers and every `[env.*]` block
are `deny_unknown_fields`, so a typo is a refusal rather than a silent no-op.

## Credentials, read from the environment

Only `mazet login` reads these, and only for the mode it is running. They are
consulted left to right; a `_FILE` spelling wins over a value spelling of the
same credential, and a `MAZET_` variable set on purpose for this command wins
over an `AZURE_` one a CI image exported for everything. An empty variable
counts as unset.

| credential | variables, highest precedence first |
|---|---|
| user password, or a service principal's client secret | `MAZET_PASSWORD_FILE` (path), `MAZET_PASSWORD` (value), `AZURE_CLIENT_SECRET` (value) |
| a PEM holding the key and the certificate | `MAZET_CERTIFICATE` (path), `AZURE_CLIENT_CERTIFICATE_PATH` (path) |
| a federated (OIDC) token | `MAZET_FEDERATED_TOKEN_FILE` (path), `MAZET_FEDERATED_TOKEN` (value), `AZURE_FEDERATED_TOKEN_FILE` (path) |

`az` is always handed a **path**, never the secret: for a value spelling,
`mazet` writes a `0600` file inside the `0700` store and deletes it when the
login is over.

One trailing line ending is dropped from a secret, and a private copy written
only when the file actually had one — a password sent with a `\n` on the end
fails as a *wrong password*, with nothing in the error pointing at why.
`MAZET_CERTIFICATE` and `AZURE_CLIENT_CERTIFICATE_PATH` are exempt: `az` opens
that path itself as a PEM, and a PEM ends in a newline by definition.

## Other variables `mazet` reads

| variable | effect |
|---|---|
| `MAZET_ENV` | selects an environment, exactly as `--env` does |
| `MAZET_DATA_DIR` | overrides the data root (the stores) |
| `MAZET_CONFIG_DIR` | overrides the config root (the registry) |
| `MAZET_AZ` | the `az` to run: a path, or a bare name to look up on `PATH` |

`MAZET_DATA_DIR` and `MAZET_CONFIG_DIR` are independent — setting one leaves
the other at its platform default.

## Variables `mazet` gives a child

Set by `mazet exec` on the child, and printed by `mazet env` for a shell to
evaluate. Nothing is ever set on the `mazet` process itself.

| variable | when |
|---|---|
| `AZURE_CONFIG_DIR` | always — it is the store |
| `ARM_TENANT_ID` | when the selected environment names a `tenant` |
| `ARM_SUBSCRIPTION_ID` | when it names a `subscription` **as a GUID** |

A variable the selected environment does not name is **removed** from the
child, and `mazet env` prints an `unset` for it.

The shell hook exports and unexports `AZURE_CONFIG_DIR` only. It keeps its own
bookkeeping in `_MAZET_HOOK`, `_MAZET_OWNED`, `_MAZET_PREV`, `_MAZET_PREV_SET`
and `_MAZET_REPORTED`, which are internal to the emitted script.

## Where mazet keeps things

| | Linux | macOS | Windows |
|---|---|---|---|
| data root (stores) | `~/.local/share/mazet` | `~/Library/Application Support/mazet` | `%APPDATA%\mazet\data` |
| config root (registry) | `~/.config/mazet` | `~/Library/Application Support/mazet` | `%APPDATA%\mazet\config` |

Resolved by the [`directories`](https://crates.io/crates/directories) crate, so
they are the platform's own answer rather than a hardcoded `~/.mazet`. Stores
live under the data root because they are state the user does not edit — `az`
writes them; the registry lives under the config root because it is a file an
operator may open.

```text
<data>/profiles/<name>/       a named profile's store
<data>/stores/<key>/          a derived store
<config>/registry.toml        the registry
```

A store directory is `0700` on Unix. It never defaults into the repository
being worked on, nor into `/tmp`.

## The registry

`<config>/registry.toml`, and it may be edited by hand.

```toml
version = 1

[profiles.client-a]
# A note to the operator about what this profile is for. It selects nothing;
# a `.mazet` does that.
tenant = "00000000-0000-0000-0000-000000000000"

# "In tenant X, I am me@corp.com." Applies when a .mazet names that tenant and
# the operator wrote no local override for it.
[identities."00000000-0000-0000-0000-000000000000"]
username = "me@corp.com"
```

An `[identities.<tenant>]` block takes `username`, `client_id`, or both.
Nothing in the registry authenticates anything.

A profile name becomes a directory name, so it is constrained rather than
trusted: at least one character, ASCII letters, digits, `.`, `-` and `_`, and
never `.` or `..`. Nothing that parses can escape the profiles directory.

## Which store a command uses

Three rules, in order, after the two layers and the registry have been folded
together:

```text
  [ 1 ]  profile = "<name>"   ──────>  <data>/profiles/<name>
  [ 2 ]  store   = "local"    ──────>  .mazet/store/
                                       (an error for the flat spelling)
  [ 3 ]  otherwise            ──────>  <data>/stores/<hint>-<hash>
```

`--profile <name>` and `--mazet <path>` on the command line select the config
to resolve; `--env <name>` then picks among the environments it declares.

### The derived store key

Rule 3's directory name is `<hint>-<16 lowercase hex digits>`.

The digits are FNV-1a (64-bit) over a canonical string beginning
`mazet-store/v1\n`, followed by whichever of these the effective config
supplied, each on its own `key=value` line and in this order:

```text
  cloud=<always, the effective cloud>
  tenant=<if named>
  subscription=<the SELECTED environment's, if named>
  username=<if named>
  client_id=<if named>
```

Whatever is absent simply does not contribute, and the key is stable across
runs. Two clones of one infra repository on a machine therefore share a login;
one operator's two identities in the same tenant do not collide, because their
local files name them.

**With no tenant, no subscription and no identity — the empty `.mazet` — the
canonical string is `path=<the config's absolute path>` instead**, and the hint
is the literal `path`. That tree gets a store of its own and keeps it on every
later call. Both spellings put the `.mazet` at the same place, so converting
one to the other does not move the store.

Otherwise the hint is a sanitized, lowercased, 16-character fragment of the
most identifying field available — tenant, else username, else client id, else
subscription. It carries no meaning the key depends on; it is there so a human
reading the stores directory can tell one from another.

FNV-1a is implemented in `resolve` rather than taken from `std::hash` because
`DefaultHasher` makes no promise of stability across Rust releases. See
[`ARCHITECTURE.md`](ARCHITECTURE.md#the-derived-store-name-is-a-hash-mazet-owns).

## Exit codes

| code | meaning |
|---|---|
| 0 | success |
| 1 | the command ran and failed |
| 2 | a usage error: an unknown flag, a missing argument, a bad value |
| 3 | the current directory is bound to no `.mazet` |

`mazet exec` is the exception: its exit status is the child's.
