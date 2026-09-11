# mazet

Run `az` under several Azure identities at once, chosen by the directory you
are standing in.

The Azure CLI keeps all of its authentication state in one place
(`AZURE_CONFIG_DIR`), so a second `az login` competes with the first: one
active identity at a time, and a service-principal login can displace a user
one. `mazet` gives each named profile its own isolated store and resolves which
profile applies from the current directory — so `az` in `~/work/client-a`
speaks as one identity and `az` in `~/work/client-b` as another, at the same
time, with no re-login between them.

## Install

Download the archive for your platform from the
[releases page](https://github.com/LeTuR/mazet/releases) and put `mazet` on
your `PATH`. Archives are published for Linux (x86-64, arm64), macOS (Intel,
Apple silicon) and Windows (x86-64), with a `sha256` checksum file beside them.

From source, with Rust 1.85 or newer:

```sh
cargo install --git https://github.com/LeTuR/mazet
```

## Commands

```sh
mazet                          # what this machine knows about
mazet profile add client-a     # register a profile with a store of its own
mazet profile list             # name, store directory, and whether it exists yet
mazet profile rm client-a      # unregister it (the store directory is kept)
```

`mazet` is an [AXI](https://axi.md) (`axi/1.0-2026-07`): output is
human-readable on a terminal and [TOON](https://github.com/toon-format/spec)
down a pipe, every help surface carries worked examples, and errors say what to
do next. Force a format with `--json`, `--pretty`, `--toon` or `--text`.

Exit codes are `0` for success, `1` for a command that ran and failed, `2` for
a usage error. A failure prints a structured `error`/`suggestion` document on
**stdout**, so a caller never has to read two streams to find the answer.

## The `.mazet` format

A directory tree declares which identity it belongs to with a `.mazet` at its
root, written either way:

```text
.mazet                 # a FILE: the shared TOML, on its own
.mazet.local           #   ...and beside it, the local override

.mazet/                # a DIRECTORY
.mazet/config.toml     #   the same shared TOML
.mazet/local.toml      #   the local override
.mazet/store/          #   this folder's own AZURE_CONFIG_DIR
```

**An empty `.mazet` is valid and useful.** Its presence alone binds that tree
to a store of its own — `az` under `folder1/` becomes a different login from
`az` under `folder2/`, and that works before a single field is filled in. Every
key below only narrows what happens inside that store.

### Layer 1 — the shared config, safe to commit

`.mazet`, or `.mazet/config.toml`. What the code operates on, and nothing about
who is operating it:

```toml
tenant = "00000000-0000-0000-0000-000000000000"  # which Entra tenant
subscription = "..."       # subscription id or name to select after login
cloud = "AzureCloud"       # AzureCloud | AzureChinaCloud | AzureUSGovernment
                           #   | AzureGermanCloud | AzureBleuCloud
method = "interactive"     # a DEFAULT: interactive | device-code
                           #   | service-principal | federated
                           #   | managed-identity

# A repository spanning several subscriptions declares them as environments.
# Each may override any of the keys above.
default_env = "dev"

[env.dev]
subscription = "00000000-0000-0000-0000-00000000dev0"

[env.prod]
subscription = "00000000-0000-0000-0000-0000000prod0"
tenant = "11111111-1111-1111-1111-111111111111"
```

A repository using one subscription writes the scalar `subscription`, no
`[env.*]` blocks, and never sees any of this.

**One store holds exactly one active subscription**, which is why environments
are stores and not a switch: `az` marks one subscription `isDefault` inside
`AZURE_CONFIG_DIR`, so a `terraform apply` against prod and an `az` query
against dev out of one store would fight over which is active. Each environment
therefore resolves to its own store.

Environment selection, highest first: `--env <name>`, then `MAZET_ENV`, then
`default_env`, then the sole `[env.*]` block when there is exactly one,
otherwise the top-level keys alone. An `--env` the config does not declare is
an error listing the ones it does.

### Layer 2 — the local override, never committed

`.mazet.local`, or `.mazet/local.toml`. Who *this* operator is:

```toml
username = "me@corp.com"   # this operator's identity within the tenant
client_id = "..."          # or their service principal / managed identity
method = "device-code"     # overrides the shared default
store = "local"            # "local" = .mazet/store/ beside this config;
                           #   "central" = a store under your data directory,
                           #   derived from the effective identity
profile = "client-a"       # use this named profile's store instead; wins over `store`
```

**A committed `.mazet` is the primary use case, and the layering is what keeps
it working for more than one person.** A shared config pinning `username` pins
the author's identity on everyone who clones the repository. So `username`,
`client_id`, `profile` and `store` are local-layer keys, and finding one of
them in the shared layer is a warning naming the key — a repository with one
operator is entitled to do it, but it has to be said out loud. `store` and
`profile` are read from one layer as a unit, so a local file spelling either of
them replaces both and a committed `profile` never overrides an operator who
opted out.

An operator who is one identity in a tenant on their own machine should not
need a local file per repository, so the central registry may carry a
per-tenant identity default. Identity precedence is **local file → registry
default → shared config**.

### Every field is optional

| absent | what happens |
|---|---|
| `tenant` | no `--tenant` on the login; you pick your tenant as `az login` already lets you |
| `subscription` | nothing is selected after login; `az`'s own default stands |
| `cloud` | `AzureCloud` |
| `method` | `interactive` |
| `default_env`, with several `[env.*]` and no selection | the top-level keys alone, and a warning naming the environments not chosen |
| every key | the store is bound to this config's location and nothing else |

An **absent** field is never an error; a field that is **present but
malformed** always is, naming the file and the key. An omission is a config
nobody filled in; a bad GUID is a typo, and ignoring it would log you into the
wrong place.

### No key ever holds a secret

A tenant, a subscription, a cloud, a username and a client id are identifiers,
and belong in a committed config. Anything that *authenticates* — a password, a
client secret, a certificate, a token — does not, and the parser refuses it
rather than storing it. Those reach `az` from the environment at login time.

**`mazet` holds paths and names; `az` holds secrets, in the directory `mazet`
points it at.** A config is safe to commit; a store never is. When `mazet`
creates a local store it writes `.mazet/.gitignore` covering `store/` and
`local.toml`; for the flat spelling it adds `.mazet.local` to the tree's own
`.gitignore`. A store directory is `0700` on Unix.

## Which store a command uses

```text
  .mazet + .mazet.local + the registry
              |
              v
  [ 1 ]  profile = "<name>"  ──────────────>  <data>/profiles/<name>
              |  no
              v
  [ 2 ]  store = "local"     ──────────────>  .mazet/store/
              |  no                            (an error for the flat spelling)
              v
  [ 3 ]  derive a key from the EFFECTIVE      <data>/stores/<hint>-<hash>
         identity: cloud, tenant, the
         selected env's subscription, and
         username or client_id.
         Nothing at all?  the config's
         own absolute path.
```

Whatever is absent simply does not contribute to the key, and the key is stable
across runs. Two clones of one infra repository on a machine therefore share a
login; one operator's two identities in the same tenant do not collide, because
their local files name them.

## Where mazet keeps things

| | Linux | macOS | Windows |
|---|---|---|---|
| stores | `~/.local/share/mazet` | `~/Library/Application Support/mazet` | `%APPDATA%\mazet\data` |
| registry | `~/.config/mazet/registry.toml` | `~/Library/Application Support/mazet/registry.toml` | `%APPDATA%\mazet\config\registry.toml` |

Both roots are overridable with `MAZET_DATA_DIR` and `MAZET_CONFIG_DIR`.

The registry is a TOML file you may edit by hand:

```toml
version = 1

[profiles.client-a]
tenant = "00000000-0000-0000-0000-000000000000"   # a note; it selects nothing

# "In tenant X, I am me@corp.com." Applies when a .mazet names that tenant and
# you wrote no local override for it.
[identities."00000000-0000-0000-0000-000000000000"]
username = "me@corp.com"
```

## Status

The crate, the pipelines and the profile model are in place. `az` interaction
(`login`, `logout`, `exec`, `env`, `status`) and directory resolution
(`init`, `use`, walking up to find a `.mazet`, the shell hooks) are next.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). The remote takes **rebase merges
only**.

## License

MIT — see [`LICENSE`](LICENSE).
